//! Does an observer move the run (openspec AG.1, AG.34, AH.40)? AF.11's
//! discriminators, the ones that told its set A (observed at the end only)
//! from its set B (paused every 60 flips) where end positions can't (set A's
//! own end spread is 1.48 m wide and splits at the gate's corner), read off a
//! reference group, a candidate group of the same script and a paused group
//! (AF.11's set B) at the end flip, and judged by one of two rules
//! ([`Choice`]), AH.40's (d) unless AG.34's (a) is asked for:
//!
//! - MT draws since F0: a pause added about 19 draws idle, and set B sat at
//!   rank-sum z +3.17 idle and +2.65 walking. Agreeing means |z| < 2, by
//!   both rules;
//! - the camera's resting offset behind the hero in x (the hero's x less the
//!   camera's): 3.05-3.23 m in AF.11's set A, 3.33-3.55 m in its set B;
//! - where the hero of the slide outcome stops, in x and in z: set B's
//!   stopped 5-7 cm short.
//!
//!   (d): the camera and the slide's z agree when the candidates' rank-sum
//!   |z| is under 2 against the reference AND 2 or more against the paused
//!   group, so the candidates sit apart from the paused group as a group,
//!   the test that tells the unwatched reference from it too (AH.5's set A
//!   against AF.11's set B at -3.25 and -2.93). The slide's x is printed and
//!   judges nothing: there set B's spread overlaps set A's own (AH.5's set A
//!   against set B at z +1.90), so it can't tell a paused group from an
//!   unwatched one.
//!
//!   (a): all three agree when the candidates' rank-sum |z| against the
//!   reference is under 2 AND no candidate lies inside the paused group's
//!   range. One replay at the edge of set B's range fails it, and the
//!   unwatched set fails it against itself: AH.5's set A held a camera
//!   (3.4182 m) and a slide's x (20.5344) inside set B's ranges, 2 of AG.34's
//!   fresh set A's 8 slides and 1 of AF.11's 7 lie inside set B's x range.
//!
//!   AG.1's letter (every candidate inside the reference's range) is still
//!   counted and printed but judges nothing: two groups of 10 from one run
//!   pass it about 1 time in 4 (both extremes of the 20 have to fall in the
//!   reference, 9/38);
//! - the gate's two outcomes: when the reference holds both, the candidates
//!   hold both, by both rules.
//!
//! AF.11 found 15 of its 20 walks slid along the corner and stopped at x
//! 20.52-20.64, z -85.12 to -85.22, and 5 turned free and stopped 1.3-1.7 m
//! further toward the bridge; a stop within [`SLIDE_RADIUS`] of
//! [`SLIDE_STOP`] on the ground is a slide, anything else turned free. The
//! walk is AF.11's ([`super::WALK`]); for another script the outcome rows say
//! nothing.

use std::fmt::Write as _;

use anyhow::{Result, bail};
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
/// Under (d), the candidates sit apart from the paused group at this rank-sum
/// |z| or more: AH.5's set O sat at -3.78 (the camera) and -3.24 (the slide's
/// z) against AF.11's set B, its unwatched set A at -3.25 and -2.93.
pub const Z_APART: f64 = 2.0;

/// Which rule judges the candidates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub enum Choice {
    /// AG.34's (a), chosen 2026-10-19: the camera's offset and both slide
    /// stops by rank-sum against the reference and no candidate inside the
    /// paused group's range.
    A,
    /// AH.40's (d), chosen 2026-10-20, what later observing checks use: the
    /// camera's offset and the slide's stop z by rank-sum against the
    /// reference and apart from the paused group by rank-sum, the slide's
    /// stop x printed only.
    #[default]
    D,
}

impl Choice {
    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "a" | "A" => Ok(Choice::A),
            "d" | "D" => Ok(Choice::D),
            other => bail!("a rule is a (AG.34's) or d (AH.40's), not {other:?}"),
        }
    }

    /// The camera's offset's, the slide's stop x's and its z's rules; the
    /// draws are [`Rule::Rank`] under both.
    fn rules(self) -> [Rule; 3] {
        match self {
            Choice::A => [Rule::RankClearOfPaused; 3],
            Choice::D => [
                Rule::RankApartFromPaused,
                Rule::Printed,
                Rule::RankApartFromPaused,
            ],
        }
    }

    fn name(self) -> &'static str {
        match self {
            Choice::A => "AG.34's rule (a)",
            Choice::D => "AH.40's rule (d)",
        }
    }
}

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

/// How one discriminator is judged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Rule {
    /// Rank-sum |z| under [`Z_AGREE`] against the reference: the draws.
    Rank,
    /// [`Rule::Rank`], and no candidate inside the paused group's [min, max]:
    /// (a)'s camera offset and slide stops. Without a paused group's values
    /// there's nothing to stay clear of, and it misses.
    RankClearOfPaused,
    /// [`Rule::Rank`], and the candidates' rank-sum |z| against the paused
    /// group at [`Z_APART`] or more: (d)'s camera offset and slide's stop z.
    /// Without a paused group's values (or with every value equal) nothing
    /// sets them apart, and it misses.
    RankApartFromPaused,
    /// Printed, judging nothing (its `agrees` always true): (d)'s slide's stop
    /// x.
    Printed,
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
    /// The same, the candidates against the paused group: (d)'s second
    /// clause, printed under (a).
    pub z_paused: Option<f64>,
    /// The reference against the paused group (printed): how far apart the
    /// unwatched group sits on its own, the separation (d) asks the
    /// candidates to keep.
    pub reference_z_paused: Option<f64>,
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
        let z_paused = rank_sum_z(&paused, &candidate);
        let by_rank =
            !reference.is_empty() && !candidate.is_empty() && z.is_none_or(|z| z.abs() < Z_AGREE);
        let inside_paused = inside(paused_band, &candidate);
        let agrees = match rule {
            Rule::Rank => by_rank,
            Rule::RankClearOfPaused => by_rank && paused_band.is_some() && inside_paused == 0,
            Rule::RankApartFromPaused => by_rank && z_paused.is_some_and(|z| z.abs() >= Z_APART),
            Rule::Printed => true,
        };
        Self {
            name: name.to_owned(),
            candidate_band: Band::of(&candidate),
            inside: inside(reference_band, &candidate),
            z_paused,
            reference_z_paused: rank_sum_z(&paused, &reference),
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
    /// The rule judging them.
    pub choice: Choice,
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
    /// `reference`, `candidate` and `paused` read at `end` and judged by
    /// `choice`; replays without a hero or a camera there are left out (and
    /// their labels with them).
    pub fn build(
        script: &str,
        reference: (Set, &[&ReplayRecord]),
        candidate: (Set, &[&ReplayRecord]),
        paused: Option<(Set, &[&ReplayRecord])>,
        end: u64,
        choice: Choice,
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
        let [camera_rule, x_rule, z_rule] = choice.rules();
        let draws = three("MT draws since F0", &|e| e.draws, Rule::Rank);
        let camera_offset = three(
            "camera's offset behind the hero in x (m)",
            &|e| Some(e.hero[0] - e.camera[0]),
            camera_rule,
        );
        let outcome = |e: &End| Outcome::of(e.hero[0], e.hero[2]);
        let slide_stop = [(0, x_rule), (2, z_rule)].map(|(i, rule)| {
            three(
                if i == 0 {
                    "slide's stop x (m)"
                } else {
                    "slide's stop z (m)"
                },
                &|e| (outcome(e) == Outcome::Slide).then_some(e.hero[i]),
                rule,
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
            choice,
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
    /// its sets apart by, and sit apart from the paused group (d) or clear of
    /// its ranges (a). A printed discriminator agrees whatever it holds.
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
            "== {}: set {} against set {} at F0 + {}, {} {paused}, AF.11's discriminators by {} \
             ({}, {} and {} replays) ==",
            self.script,
            self.candidate,
            self.reference,
            self.end,
            match self.choice {
                Choice::A => "clear of",
                Choice::D => "apart from",
            },
            self.choice.name(),
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
            let reference = self.reference;
            let rule = match d.rule {
                Rule::Rank => format!("rank-sum |z| < {Z_AGREE}"),
                Rule::RankClearOfPaused => {
                    format!("rank-sum |z| < {Z_AGREE} and none inside {paused}'s range")
                }
                Rule::RankApartFromPaused => format!(
                    "rank-sum |z| < {Z_AGREE} against {reference} and |z| >= {Z_APART} against \
                     {paused}"
                ),
                Rule::Printed => "printed".to_owned(),
            };
            let verdict = match (d.rule, d.agrees) {
                (Rule::Printed, _) => "judges nothing",
                (_, true) => "agrees",
                (_, false) => "MISSES",
            };
            let _ = writeln!(
                out,
                "{}: {reference} {}; {} {}; {paused} {}; rank-sum z {} against {reference}, {} \
                 against {paused} ({reference} against {paused} {}); {} of {} inside \
                 {reference}'s range, {} inside {paused}'s; {rule} -> {verdict}",
                d.name,
                band(&d.reference_band),
                self.candidate,
                band(&d.candidate_band),
                band(&d.paused_band),
                z(d.z),
                z(d.z_paused),
                z(d.reference_z_paused),
                d.inside,
                d.candidate.len(),
                d.inside_paused,
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
        choice: Choice,
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
            choice,
        )
    }

    /// Against the miniature set A, clear of the miniature set B, by (a).
    fn judge(w: &[ReplayRecord]) -> Discriminators {
        judge_against(&set_a(), w, Some(&set_b()), Choice::A)
    }

    /// The same groups by (d).
    fn judge_d(w: &[ReplayRecord]) -> Discriminators {
        judge_against(&set_a(), w, Some(&set_b()), Choice::D)
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
        // By (d) too: as far from set B as set A is.
        let d = judge_d(&candidates(&like));
        assert!(d.agree(), "{d:#?}");
        assert_eq!(d.choice, Choice::D);
        for (r, rule) in [
            (&d.camera_offset, Rule::RankApartFromPaused),
            (&d.slide_stop[0], Rule::Printed),
            (&d.slide_stop[1], Rule::RankApartFromPaused),
        ] {
            // The slides' x sit above set B's here, the camera and z below.
            assert_eq!(r.rule, rule);
            assert!(r.z_paused.unwrap().abs() >= Z_APART, "{r:#?}");
            assert!(r.reference_z_paused.unwrap().abs() >= Z_APART, "{r:#?}");
        }
        assert!(d.camera_offset.reference_z_paused.unwrap() < 0.0);
        assert!(d.slide_stop[1].reference_z_paused.unwrap() < 0.0);
        // The reference against the paused group, not the other way round.
        assert_eq!(
            d.camera_offset.reference_z_paused,
            rank_sum_z(&d.camera_offset.paused, &d.camera_offset.reference)
        );
        let mut text = String::new();
        d.write_text(&mut text);
        assert!(text.contains("apart from set B"), "{text}");
        assert!(text.contains("by AH.40's rule (d)"), "{text}");
        assert!(text.contains("(A against set B -3.78)"), "{text}");
        assert!(
            text.contains("|z| < 2 against A and |z| >= 2 against set B -> agrees"),
            "{text}"
        );
        assert!(text.contains("printed -> judges nothing"), "{text}");
        assert!(text.contains("every discriminator agrees"), "{text}");
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
            Choice::A,
        );
        assert!(d.outcomes.agrees);
        // A replay without a camera at the end is left out.
        let mut w = candidates(&like);
        w[3].observations[1].camera = None;
        assert_eq!(judge(&w).candidate_labels.len(), 9);
    }

    #[test]
    fn without_a_paused_group_the_ranges_cant_agree() {
        let d = judge_against(&set_a(), &candidates(&like), None, Choice::A);
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
        let d = judge_against(&set_a(), &candidates(&like), Some(&free), Choice::A);
        assert!(d.camera_offset.agrees);
        assert!(
            d.slide_stop
                .iter()
                .all(|s| !s.agrees && s.paused.is_empty())
        );
    }

    #[test]
    fn a_rule_is_a_or_d_and_d_is_the_default() {
        assert_eq!(Choice::default(), Choice::D);
        for (text, choice) in [
            ("a", Choice::A),
            ("A", Choice::A),
            ("d", Choice::D),
            ("D", Choice::D),
        ] {
            assert_eq!(Choice::parse(text).unwrap(), choice);
        }
        for text in ["", "b", "c", "(d)", "ad", "rank"] {
            assert!(Choice::parse(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn rule_d_judges_by_rank_where_one_replay_at_set_bs_edge_fails_rule_a() {
        // AH.5's walk-O-03, a slide's x inside set B's x range, and its set A's
        // walk-A-05, a camera inside set B's offsets: each group still far
        // from set B by rank.
        let w = candidates(&|i| {
            let (mut h, o, d) = like(i);
            if i == 3 {
                h[0] = 20.55;
            }
            (h, if i == 4 { 3.34 } else { o }, d)
        });
        let a = judge(&w);
        assert!(!a.camera_offset.agrees && !a.slide_stop[0].agrees && !a.agree());
        assert!(a.slide_stop[1].agrees && a.draws.agrees && a.outcomes.agrees);
        let d = judge_d(&w);
        assert!(d.agree(), "{d:#?}");
        // Still counted and printed.
        assert_eq!(
            (d.camera_offset.inside_paused, d.slide_stop[0].inside_paused),
            (1, 1)
        );
        assert!(d.camera_offset.z.unwrap().abs() < Z_AGREE);
        assert!(d.camera_offset.z_paused.unwrap() <= -Z_APART);
        assert!(d.slide_stop[1].z_paused.unwrap() <= -Z_APART);
    }

    #[test]
    fn under_rule_d_the_slides_x_judges_nothing() {
        // Every slide 20 cm past set A's in x: (a) misses by rank, (d) prints it.
        let w = candidates(&|i| {
            let (mut h, o, d) = like(i);
            if Outcome::of(f64::from(h[0]), f64::from(h[2])) == Outcome::Slide {
                h[0] += 0.2;
            }
            (h, o, d)
        });
        let a = judge(&w);
        assert!(!a.slide_stop[0].agrees && a.slide_stop[0].z.unwrap() > Z_AGREE);
        let d = judge_d(&w);
        assert_eq!(d.slide_stop[0].rule, Rule::Printed);
        assert!(d.slide_stop[0].z.unwrap() > Z_AGREE);
        assert_eq!(d.slide_stop[0].candidate.len(), 8);
        assert!(d.slide_stop[0].agrees && d.agree(), "{d:#?}");
        let mut text = String::new();
        d.write_text(&mut text);
        let line = text
            .lines()
            .find(|l| l.starts_with("slide's stop x"))
            .unwrap();
        assert!(line.ends_with("printed -> judges nothing"), "{line}");
    }

    #[test]
    fn apart_from_the_paused_group_means_a_rank_sum_z_of_2_or_more() {
        // The paused group 1..10; three candidates topping `tops` of its values
        // each and seven under them all, so U (the pairs a candidate tops) is
        // their sum; the reference each candidate 1 cm up, |z| under 0.4.
        let paused: Vec<f64> = (1..=10).map(f64::from).collect();
        let group = |tops: [u32; 3], sign: f64| -> (Vec<f64>, Vec<f64>) {
            let mut c: Vec<f64> = tops.iter().map(|&t| f64::from(t) + 0.5).collect();
            c.extend((0..7).map(|i| -1.0 - f64::from(i)));
            let r = c.iter().map(|v| (v + 0.01) * sign).collect();
            (r, c.iter().map(|v| v * sign).collect())
        };
        let judge = |tops, sign: f64, paused: &[f64]| {
            let (r, c) = group(tops, sign);
            let p = paused.iter().map(|v| v * sign).collect();
            Discriminator::new("x", r, c, p, Rule::RankApartFromPaused)
        };
        for sign in [1.0, -1.0] {
            // U 23: z -2.04, apart; U 24: z -1.97, not (signs flipped above).
            let d = judge([10, 9, 4], sign, &paused);
            assert!((d.z_paused.unwrap() + 2.041 * sign).abs() < 1e-3, "{d:#?}");
            assert!(d.z.unwrap().abs() < 0.4 && d.agrees, "{d:#?}");
            let d = judge([10, 9, 5], sign, &paused);
            assert!((d.z_paused.unwrap() + 1.965 * sign).abs() < 1e-3, "{d:#?}");
            assert!(!d.agrees, "{d:#?}");
        }
        // The reference's own z against the paused group, printed: here the
        // candidates' less the 1 cm.
        let d = judge([10, 9, 4], 1.0, &paused);
        assert!((d.reference_z_paused.unwrap() + 2.041).abs() < 1e-3);
        // Apart from the paused group but off the reference: a miss by rank.
        let (r, c) = group([10, 9, 4], 1.0);
        let d = Discriminator::new(
            "x",
            r.iter().map(|v| v + 100.0).collect(),
            c,
            paused.clone(),
            Rule::RankApartFromPaused,
        );
        assert!(d.z.unwrap() < -Z_AGREE && !d.agrees);
        // No paused group, or every value equal: nothing sets them apart.
        let (r, c) = group([1, 0, 0], 1.0);
        let d = Discriminator::new("x", r, c, vec![], Rule::RankApartFromPaused);
        assert!(d.z_paused.is_none() && d.reference_z_paused.is_none() && !d.agrees);
        let d = Discriminator::new(
            "x",
            vec![3.1; 10],
            vec![3.1; 10],
            vec![3.1; 10],
            Rule::RankApartFromPaused,
        );
        assert!(d.z.is_none() && d.z_paused.is_none() && !d.agrees);
        // A printed one agrees whatever it holds.
        let d = Discriminator::new("x", vec![1.0; 5], vec![9.0; 5], vec![], Rule::Printed);
        assert!(d.z.unwrap() > Z_AGREE && d.agrees);
    }

    #[test]
    fn under_rule_d_a_watched_set_paused_like_set_b_misses() {
        // A reference and candidates that both sit with set B (as AF.11's set B
        // judged against itself): nothing tells them apart by rank, and (d)
        // misses on the camera and the slide's z, where the slide's x would
        // have said nothing either way.
        let as_set = |set: Set| -> Vec<ReplayRecord> {
            set_b()
                .into_iter()
                .map(|mut r| {
                    r.set = set;
                    r
                })
                .collect()
        };
        let d = judge_against(&as_set(Set::A), &as_set(Set::W), Some(&set_b()), Choice::D);
        assert!(d.draws.agrees && d.outcomes.agrees);
        assert!(d.camera_offset.z.unwrap().abs() < Z_AGREE);
        assert!(d.camera_offset.z_paused.unwrap().abs() < Z_APART);
        assert!(!d.camera_offset.agrees && !d.slide_stop[1].agrees);
        assert!(d.slide_stop[0].agrees && !d.agree());
        // Without a paused group: the camera and the slide's z miss, the
        // slide's x judges nothing.
        let d = judge_against(&set_a(), &candidates(&like), None, Choice::D);
        assert!(d.draws.agrees && d.outcomes.agrees && d.slide_stop[0].agrees);
        assert!(!d.camera_offset.agrees && !d.slide_stop[1].agrees && !d.agree());
        let mut text = String::new();
        d.write_text(&mut text);
        assert!(text.contains("apart from no paused set"), "{text}");
        // The draws by rank alone, as under (a).
        let d = judge_d(&candidates(&|i| {
            let (h, o, d) = like(i);
            (h, o, d + 3_000)
        }));
        assert!(!d.draws.agrees && d.draws.rule == Rule::Rank && !d.agree());
        // Both outcomes asked of the candidates when the reference has both.
        let d = judge_d(&candidates(&|i| {
            let (_, o, d) = like(i);
            (SLIDE, o, d)
        }));
        assert!(!d.outcomes.agrees && !d.agree());
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
        assert_eq!((o.paused, o.choice), (None, Choice::D));
        assert!(!o.agree());
        // Another measurement's set B (AF.11's, whose set A is ignored).
        let mut af11 = set_b();
        af11.extend(w().into_iter().map(|mut r| {
            r.set = Set::A;
            r.observations[1].camera.as_mut().unwrap().position[0] = 0.0;
            r
        }));
        let report =
            crate::divergence::Report::build_with(&records, None, &af11, Choice::A).unwrap();
        let o = &report.observing[0];
        assert_eq!((o.paused, o.paused_labels.len()), (Some(Set::B), 10));
        assert!(o.agree(), "{o:#?}");
        assert!(
            report
                .text()
                .contains("set W against set A at F0 + 1800, clear of set B")
        );
        assert_eq!(o.slide_stop[0].rule, Rule::RankClearOfPaused);
        // The same by (d), the rule reaching every discriminator.
        let report =
            crate::divergence::Report::build_with(&records, None, &af11, Choice::D).unwrap();
        let o = &report.observing[0];
        assert_eq!((o.choice, o.slide_stop[0].rule), (Choice::D, Rule::Printed));
        assert_eq!(o.camera_offset.rule, Rule::RankApartFromPaused);
        assert!(o.agree(), "{o:#?}");
        assert!(
            report
                .text()
                .contains("set W against set A at F0 + 1800, apart from set B")
        );
        // A set O is judged as a set W is.
        let chained: Vec<ReplayRecord> = set_a()
            .into_iter()
            .chain(w().into_iter().map(|mut r| {
                r.set = Set::O;
                r
            }))
            .collect();
        let report =
            crate::divergence::Report::build_with(&chained, None, &af11, Choice::D).unwrap();
        let o = &report.observing[0];
        assert_eq!((o.candidate, o.paused), (Set::O, Some(Set::B)));
        assert!(o.agree(), "{o:#?}");
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
        for choice in [Choice::A, Choice::D] {
            let report = crate::divergence::Report::build_with(&own, None, &near, choice).unwrap();
            let o = &report.observing[0];
            assert!(o.agree(), "{o:#?}");
            assert!(o.camera_offset.paused_band.unwrap().min > 4.0);
        }
        // No set W, no judging.
        let report =
            crate::divergence::Report::build_with(&set_a(), None, &set_b(), Choice::D).unwrap();
        assert!(report.observing.is_empty());
    }
}
