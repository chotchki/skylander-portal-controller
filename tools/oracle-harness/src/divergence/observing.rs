//! Does an observer move the run (openspec AG.1, AG.34)? AF.11's
//! discriminators, the ones that told its set A (observed at the end only)
//! from its set B (paused every 60 flips) where end positions can't (set A's
//! own end spread is 1.48 m wide and splits at the gate's corner), read off a
//! reference group, a candidate group of the same script and a paused group
//! (AF.11's set B) at the end flip, and judged by AG.34's rule (a):
//!
//! - MT draws since F0: a pause added about 19 draws idle, and set B sat at
//!   rank-sum z +3.17 idle and +2.65 walking. Agreeing means |z| < 2;
//! - the camera's resting offset behind the hero in x (the hero's x less the
//!   camera's): 3.05-3.23 m in AF.11's set A, 3.33-3.55 m in its set B;
//! - where the hero of the slide outcome stops, in x and in z: set B's
//!   stopped 5-7 cm short.
//!
//!   Each of these three agrees when the candidates' rank-sum |z| against the
//!   reference is under 2 AND no candidate lies inside the paused group's
//!   range. AG.1's letter (every candidate inside the reference's range) is
//!   still counted and printed but judges nothing: two groups of 10 from one
//!   run pass it about 1 time in 4 (both extremes of the 20 have to fall in
//!   the reference, 9/38). On the slides' x the clearance is weak: set B's x
//!   range overlaps set A's own (2 of AG.34's fresh set A's 8 slides and 1 of
//!   AF.11's 7 lie inside it), so z and the camera carry the separation;
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
/// Every discriminator agrees under this rank-sum |z|: about a 5 % chance
/// that two groups of one run's replays land past it, where AF.11's pauses put
/// set B at +2.65 walking and +3.17 idle on draws, +3.78 on the camera's
/// offset and -3.24 and +3.24 on the slides' stops (against AG.1's fresh set
/// A).
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

/// How a discriminator is judged (AG.34's rule (a)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Rule {
    /// Rank-sum |z| under [`Z_AGREE`] against the reference: the draws.
    Rank,
    /// [`Rule::Rank`], and no candidate inside the paused group's [min, max]:
    /// the camera's offset and the slides' stops. Without a paused group's
    /// values there's nothing to stay clear of, and it misses.
    RankClearOfPaused,
}

/// One discriminator over the groups.
#[derive(Debug, Clone, Serialize)]
pub struct Discriminator {
    pub name: String,
    /// Each replay's value, in label order.
    pub reference: Vec<f64>,
    pub candidate: Vec<f64>,
    /// The paused group's (AF.11's set B), empty without one.
    pub paused: Vec<f64>,
    pub reference_band: Option<Band>,
    pub candidate_band: Option<Band>,
    pub paused_band: Option<Band>,
    /// Candidates inside the reference's [min, max]: AG.1's letter, printed,
    /// judging nothing.
    pub inside: usize,
    /// Candidates inside the paused group's [min, max].
    pub inside_paused: usize,
    /// Mann-Whitney's z, the candidates against the reference: positive when
    /// the candidates sit higher, `None` when every value is equal.
    pub z: Option<f64>,
    /// The same, the candidates against the paused group (printed).
    pub z_paused: Option<f64>,
    pub rule: Rule,
    pub agrees: bool,
}

/// How many of `values` lie inside `band`'s [min, max].
fn inside(band: Option<Band>, values: &[f64]) -> usize {
    band.map_or(0, |b| {
        values.iter().filter(|&&v| v >= b.min && v <= b.max).count()
    })
}

impl Discriminator {
    fn new(
        name: &str,
        reference: Vec<f64>,
        candidate: Vec<f64>,
        paused: Vec<f64>,
        rule: Rule,
    ) -> Self {
        let reference_band = Band::of(&reference);
        let paused_band = Band::of(&paused);
        let z = rank_sum_z(&reference, &candidate);
        let by_rank =
            !reference.is_empty() && !candidate.is_empty() && z.is_none_or(|z| z.abs() < Z_AGREE);
        let inside_paused = inside(paused_band, &candidate);
        let agrees = match rule {
            Rule::Rank => by_rank,
            Rule::RankClearOfPaused => by_rank && paused_band.is_some() && inside_paused == 0,
        };
        Self {
            name: name.to_owned(),
            candidate_band: Band::of(&candidate),
            inside: inside(reference_band, &candidate),
            z_paused: rank_sum_z(&paused, &candidate),
            reference,
            candidate,
            paused,
            reference_band,
            paused_band,
            inside_paused,
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

/// A candidate group against a reference group at the end flip, kept clear of
/// a paused group.
#[derive(Debug, Clone, Serialize)]
pub struct Discriminators {
    pub script: String,
    pub reference: Set,
    pub candidate: Set,
    /// The paused group's set (AF.11's B), `None` without one.
    pub paused: Option<Set>,
    pub end: u64,
    pub reference_labels: Vec<String>,
    pub candidate_labels: Vec<String>,
    pub paused_labels: Vec<String>,
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
    /// `reference`, `candidate` and `paused` read at `end`; replays without a
    /// hero or a camera there are left out (and their labels with them).
    pub fn build(
        script: &str,
        reference: (Set, &[&ReplayRecord]),
        candidate: (Set, &[&ReplayRecord]),
        paused: Option<(Set, &[&ReplayRecord])>,
        end: u64,
    ) -> Self {
        let ends = |rs: &[&ReplayRecord]| -> Vec<End> {
            let mut v: Vec<End> = rs.iter().filter_map(|r| end_of(r, end)).collect();
            v.sort_by(|a, b| a.label.cmp(&b.label));
            v
        };
        let (re, ce) = (ends(reference.1), ends(candidate.1));
        let pe = paused.map_or_else(Vec::new, |(_, rs)| ends(rs));
        let col = |v: &[End], f: &dyn Fn(&End) -> Option<f64>| -> Vec<f64> {
            v.iter().filter_map(f).collect()
        };
        let three = |name: &str, f: &dyn Fn(&End) -> Option<f64>, rule: Rule| {
            Discriminator::new(name, col(&re, f), col(&ce, f), col(&pe, f), rule)
        };
        let draws = three("MT draws since F0", &|e| e.draws, Rule::Rank);
        let camera_offset = three(
            "camera's offset behind the hero in x (m)",
            &|e| Some(e.hero[0] - e.camera[0]),
            Rule::RankClearOfPaused,
        );
        let outcome = |e: &End| Outcome::of(e.hero[0], e.hero[2]);
        let slide_stop = [0, 2].map(|i| {
            three(
                if i == 0 {
                    "slide's stop x (m)"
                } else {
                    "slide's stop z (m)"
                },
                &|e| (outcome(e) == Outcome::Slide).then_some(e.hero[i]),
                Rule::RankClearOfPaused,
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
        let labels = |v: &[End]| v.iter().map(|e| e.label.clone()).collect();
        Self {
            script: script.to_owned(),
            reference: reference.0,
            candidate: candidate.0,
            paused: paused.map(|(set, _)| set),
            end,
            reference_labels: labels(&re),
            candidate_labels: labels(&ce),
            paused_labels: labels(&pe),
            draws,
            camera_offset,
            slide_stop,
            outcomes,
        }
    }

    /// The candidates can't be told from the reference on anything AF.11 told
    /// its sets apart by, and sit clear of the paused group on the ranges.
    pub fn agree(&self) -> bool {
        self.draws.agrees
            && self.camera_offset.agrees
            && self.slide_stop.iter().all(|d| d.agrees)
            && self.outcomes.agrees
    }

    pub fn write_text(&self, out: &mut String) {
        let paused = self
            .paused
            .map_or("no paused set".to_owned(), |p| format!("set {p}"));
        let _ = writeln!(
            out,
            "== {}: set {} against set {} at F0 + {}, clear of {paused}, AF.11's discriminators \
             by AG.34's rule (a) ({}, {} and {} replays) ==",
            self.script,
            self.candidate,
            self.reference,
            self.end,
            self.candidate_labels.len(),
            self.reference_labels.len(),
            self.paused_labels.len()
        );
        let band = |b: &Option<Band>| {
            b.map_or("none".into(), |b| {
                format!("{:.4}..{:.4} median {:.4}", b.min, b.max, b.median)
            })
        };
        let z = |z: Option<f64>| z.map_or("n/a".into(), |z| format!("{z:+.2}"));
        for d in [&self.draws, &self.camera_offset]
            .into_iter()
            .chain(self.slide_stop.iter())
        {
            let rule = match d.rule {
                Rule::Rank => format!("rank-sum |z| < {Z_AGREE}"),
                Rule::RankClearOfPaused => {
                    format!("rank-sum |z| < {Z_AGREE} and none inside {paused}'s range")
                }
            };
            let _ = writeln!(
                out,
                "{}: {} {}; {} {}; {paused} {}; rank-sum z {} against {}, {} against {paused}; \
                 {} of {} inside {}'s range, {} inside {paused}'s; {rule} -> {}",
                d.name,
                self.reference,
                band(&d.reference_band),
                self.candidate,
                band(&d.candidate_band),
                band(&d.paused_band),
                z(d.z),
                self.reference,
                z(d.z_paused),
                d.inside,
                d.candidate.len(),
                self.reference,
                d.inside_paused,
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

    /// AF.11's set B in miniature: the camera 3.33-3.55 m back, its 8 slides
    /// stopping 3-8 cm short of set A's in x and 5-8 cm in z, more draws.
    fn set_b() -> Vec<ReplayRecord> {
        (0..10)
            .map(|i| {
                let hero = if [0, 9].contains(&i) {
                    FREE
                } else {
                    [20.53 + 0.005 * i as f32, 43.5, -85.145 + 0.003 * i as f32]
                };
                replay(
                    Set::B,
                    i,
                    hero,
                    3.33 + 0.024 * i as f32,
                    33_800 + 900 * u64::from(i),
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

    fn candidates(f: &dyn Fn(u32) -> ([f32; 3], f32, u64)) -> Vec<ReplayRecord> {
        (0..10)
            .map(|i| {
                let (h, o, d) = f(i);
                replay(Set::W, i, h, o, d)
            })
            .collect()
    }

    fn judge_against(
        a: &[ReplayRecord],
        w: &[ReplayRecord],
        b: Option<&[ReplayRecord]>,
    ) -> Discriminators {
        fn refs(v: &[ReplayRecord]) -> Vec<&ReplayRecord> {
            v.iter().collect()
        }
        let (a, w) = (refs(a), refs(w));
        let b = b.map(refs);
        Discriminators::build(
            "walk",
            (Set::A, &a),
            (Set::W, &w),
            b.as_deref().map(|b| (Set::B, b)),
            1_800,
        )
    }

    /// Against the miniature set A, clear of the miniature set B.
    fn judge(w: &[ReplayRecord]) -> Discriminators {
        judge_against(&set_a(), w, Some(&set_b()))
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
    fn a_candidate_like_the_reference_and_clear_of_set_b_agrees_on_every_discriminator() {
        // Interleaved draws, offsets inside A's, slides inside A's, both
        // outcomes, nothing inside set B's ranges.
        let d = judge(&candidates(&like));
        assert!(d.agree(), "{d:#?}");
        assert_eq!(d.camera_offset.inside, 10);
        assert!(d.draws.z.unwrap().abs() < 0.5);
        assert_eq!(d.slide_stop[0].candidate.len(), 8);
        assert_eq!(d.slide_stop[0].paused.len(), 8);
        for r in [&d.camera_offset, &d.slide_stop[0], &d.slide_stop[1]] {
            assert_eq!((r.rule, r.inside_paused), (Rule::RankClearOfPaused, 0));
            assert!(r.paused_band.is_some() && r.z_paused.is_some());
        }
        assert_eq!(d.draws.rule, Rule::Rank);
        assert_eq!((d.paused, d.paused_labels.len()), (Some(Set::B), 10));
        let mut text = String::new();
        d.write_text(&mut text);
        assert!(text.contains("every discriminator agrees"), "{text}");
        assert!(text.contains("clear of set B"), "{text}");
        assert!(text.contains("none inside set B's range"), "{text}");
    }

    #[test]
    fn a_candidate_past_the_references_range_but_clear_of_set_b_agrees() {
        // AG.1's walk-W-06: the camera 3.4 cm under the reference's least, and
        // a slide 1 cm past its x: AG.1's letter misses, rule (a) doesn't.
        let d = judge(&candidates(&|i| {
            let (mut h, o, d) = like(i);
            if i == 9 {
                h[0] = 20.70;
            }
            (h, if i == 6 { 3.016 } else { o }, d)
        }));
        assert_eq!(d.camera_offset.inside, 9);
        assert_eq!(d.slide_stop[0].inside, 7);
        assert!(d.agree(), "{d:#?}");
    }

    #[test]
    fn each_discriminator_misses_on_its_own() {
        // More draws, as a pause every 60 flips adds: z past 2.
        let d = judge(&candidates(&|i| {
            let (h, o, d) = like(i);
            (h, o, d + 3_000)
        }));
        assert!(!d.draws.agrees && d.draws.z.unwrap() > 2.0);
        assert!(d.camera_offset.agrees && d.outcomes.agrees);
        // One camera resting as far back as set B's: inside its range, a miss
        // though the rank-sum can't see it.
        let d = judge(&candidates(&|i| {
            let (h, o, d) = like(i);
            (h, if i == 4 { 3.4 } else { o }, d)
        }));
        assert!(!d.camera_offset.agrees);
        assert_eq!(d.camera_offset.inside_paused, 1);
        assert!(d.camera_offset.z.unwrap().abs() < Z_AGREE);
        assert!(d.draws.agrees && d.slide_stop.iter().all(|s| s.agrees));
        // Every camera 10 cm further back, all short of set B's: the rank-sum
        // sees it.
        let d = judge(&candidates(&|i| {
            let (h, o, d) = like(i);
            (h, o + 0.101, d)
        }));
        assert!(!d.camera_offset.agrees && d.camera_offset.z.unwrap() > Z_AGREE);
        assert_eq!(d.camera_offset.inside_paused, 0);
        // A slide stopping 6 cm short in z, inside set B's z.
        let d = judge(&candidates(&|i| {
            let (mut h, o, d) = like(i);
            if i == 7 {
                h[2] += 0.06;
            }
            (h, o, d)
        }));
        assert!(d.slide_stop[0].agrees && !d.slide_stop[1].agrees);
        assert_eq!(d.slide_stop[1].inside_paused, 1);
        // Only slides where the reference has both.
        let d = judge(&candidates(&|i| {
            let (_, o, d) = like(i);
            (SLIDE, o, d)
        }));
        assert!(!d.outcomes.agrees && !d.agree());
        // A reference with one outcome asks nothing of the candidates' outcomes.
        let slides: Vec<ReplayRecord> = (0..10)
            .map(|i| replay(Set::A, i, SLIDE, 3.1, 32_000 + u64::from(i)))
            .collect();
        let d = judge_against(
            &slides,
            &candidates(&|i| (SLIDE, 3.1, 32_000 + u64::from(i))),
            Some(&set_b()),
        );
        assert!(d.outcomes.agrees);
        // A replay without a camera at the end is left out.
        let mut w = candidates(&like);
        w[3].observations[1].camera = None;
        assert_eq!(judge(&w).candidate_labels.len(), 9);
    }

    #[test]
    fn without_a_paused_group_the_ranges_cant_agree() {
        let d = judge_against(&set_a(), &candidates(&like), None);
        assert!(d.draws.agrees && d.outcomes.agrees);
        for r in [&d.camera_offset, &d.slide_stop[0], &d.slide_stop[1]] {
            assert!(!r.agrees && r.paused.is_empty() && r.paused_band.is_none());
        }
        assert!(!d.agree() && d.paused.is_none() && d.paused_labels.is_empty());
        let mut text = String::new();
        d.write_text(&mut text);
        assert!(text.contains("clear of no paused set"), "{text}");
        // A paused group with no slides leaves the slides nothing to clear.
        let free: Vec<ReplayRecord> = (0..10)
            .map(|i| replay(Set::B, i, FREE, 3.4, 34_000))
            .collect();
        let d = judge_against(&set_a(), &candidates(&like), Some(&free));
        assert!(d.camera_offset.agrees);
        assert!(
            d.slide_stop
                .iter()
                .all(|s| !s.agrees && s.paused.is_empty())
        );
    }

    #[test]
    fn the_report_judges_set_w_against_set_a_clear_of_set_b() {
        let w = || candidates(&like);
        let mut records = set_a();
        records.extend(w());
        // No set B anywhere: judged, and the ranges miss.
        let report = crate::divergence::Report::build(&records, None).unwrap();
        assert_eq!(report.observing.len(), 1);
        let o = &report.observing[0];
        assert_eq!((o.reference, o.candidate, o.end), (Set::A, Set::W, 1_800));
        assert_eq!(o.paused, None);
        assert!(!o.agree());
        // Another measurement's set B (AF.11's, whose set A is ignored).
        let mut af11 = set_b();
        af11.extend(w().into_iter().map(|mut r| {
            r.set = Set::A;
            r.observations[1].camera.as_mut().unwrap().position[0] = 0.0;
            r
        }));
        let report = crate::divergence::Report::build_with(&records, None, &af11).unwrap();
        let o = &report.observing[0];
        assert_eq!((o.paused, o.paused_labels.len()), (Some(Set::B), 10));
        assert!(o.agree(), "{o:#?}");
        assert!(
            report
                .text()
                .contains("set W against set A at F0 + 1800, clear of set B")
        );
        // The measurement's own set B comes first.
        let mut own = records.clone();
        own.extend(set_b().into_iter().map(|mut r| {
            r.observations[1].camera.as_mut().unwrap().position[0] -= 1.0;
            r
        }));
        let near: Vec<ReplayRecord> = w()
            .into_iter()
            .map(|mut r| {
                r.set = Set::B;
                r
            })
            .collect();
        let report = crate::divergence::Report::build_with(&own, None, &near).unwrap();
        let o = &report.observing[0];
        assert!(o.agree(), "{o:#?}");
        assert!(o.camera_offset.paused_band.unwrap().min > 4.0);
        // No set W, no judging.
        let report = crate::divergence::Report::build_with(&set_a(), None, &set_b()).unwrap();
        assert!(report.observing.is_empty());
    }
}
