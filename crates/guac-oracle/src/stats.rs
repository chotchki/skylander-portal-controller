//! The distribution tests (openspec AH.6, design decision 18): what a fight's
//! comparison judges where single events can't be, rolls having timed or
//! valued them. A metric's samples from guacamole's seeded runs are judged
//! against the oracle's reseeded replays: continuous values (alert waits, time
//! between bites, time to clear, damage taken) by the rank-sum, discrete ones
//! (a bite's value) by chi-square, and a list of metrics at one family-wise
//! level by Bonferroni's correction, so five metrics at 0.05 fail one at
//! |z| >= 2.58 rather than 1.96. A reference is checked against itself first:
//! each replay left out and judged against the rest at the same threshold.
//!
//! The rank-sum is AF.11's (the harness's divergence report and AG.34's rule
//! read this one): Mann-Whitney's U as a normal z, ties at their mean rank and
//! no tie correction, so heavily tied samples read a little conservative.
//! The normal and chi-square tails are both the regularized incomplete gamma
//! function, by its series and continued fraction over Lanczos' log-gamma,
//! good to about 1e-14, far tighter than any threshold here needs.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::MetricError;

/// Ranks from 1, ties sharing their mean rank.
pub fn ranks(values: &[f64]) -> Vec<f64> {
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
    let mut out = vec![0.0; values.len()];
    let mut i = 0;
    while i < order.len() {
        let mut j = i;
        while j + 1 < order.len() && values[order[j + 1]] == values[order[i]] {
            j += 1;
        }
        let rank = (i + j) as f64 / 2.0 + 1.0;
        for &k in &order[i..=j] {
            out[k] = rank;
        }
        i = j + 1;
    }
    out
}

/// The Mann-Whitney rank-sum z of `b` against `a` (normal approximation, no
/// tie correction): positive when B sits higher. `None` when either side is
/// empty or every value is equal.
pub fn rank_sum_z(a: &[f64], b: &[f64]) -> Option<f64> {
    if a.is_empty() || b.is_empty() {
        return None;
    }
    let all: Vec<f64> = a.iter().chain(b).copied().collect();
    if all.iter().all(|v| *v == all[0]) {
        return None;
    }
    let r = ranks(&all);
    let (na, nb) = (a.len() as f64, b.len() as f64);
    let rb: f64 = r[a.len()..].iter().sum();
    let u = rb - nb * (nb + 1.0) / 2.0;
    let mean = na * nb / 2.0;
    let sd = (na * nb * (na + nb + 1.0) / 12.0).sqrt();
    Some((u - mean) / sd)
}

/// P(Z > z) for a standard normal Z: half of Q(1/2, z^2 / 2), the same
/// incomplete gamma the chi-square tail uses (erfc(x) = Q(1/2, x^2)).
pub fn normal_upper(z: f64) -> f64 {
    let half = 0.5 * gamma_upper(0.5, 0.5 * z * z);
    if z >= 0.0 { half } else { 1.0 - half }
}

/// P(|Z| >= |z|): a rank-sum z's two-sided p.
pub fn two_sided_p(z: f64) -> f64 {
    (2.0 * normal_upper(z.abs())).min(1.0)
}

/// The z with P(Z <= z) = `p`, by bisection on [`normal_upper`]; `p` in (0, 1).
pub fn normal_quantile(p: f64) -> Result<f64, MetricError> {
    if !(p > 0.0 && p < 1.0) {
        return Err(MetricError::BadProbability { index: 0, value: p });
    }
    let (mut low, mut high) = (-40.0f64, 40.0f64);
    for _ in 0..200 {
        let mid = 0.5 * (low + high);
        if 1.0 - normal_upper(mid) < p {
            low = mid;
        } else {
            high = mid;
        }
    }
    Ok(0.5 * (low + high))
}

/// ln Γ(x) for x >= 0.5, Lanczos' approximation (g = 7, nine terms).
fn ln_gamma(x: f64) -> f64 {
    const G: f64 = 7.0;
    const P: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    let z = x - 1.0;
    let mut sum = P[0];
    for (i, p) in P.iter().enumerate().skip(1) {
        sum += p / (z + i as f64);
    }
    let t = z + G + 0.5;
    0.5 * (2.0 * std::f64::consts::PI).ln() + (z + 0.5) * t.ln() - t + sum.ln()
}

/// Q(a, x), the regularized upper incomplete gamma function, a >= 0.5.
fn gamma_upper(a: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 1.0;
    }
    let front = (-x + a * x.ln() - ln_gamma(a)).exp();
    if x < a + 1.0 {
        // The series for P(a, x).
        let (mut ap, mut del) = (a, 1.0 / a);
        let mut sum = del;
        for _ in 0..10_000 {
            ap += 1.0;
            del *= x / ap;
            sum += del;
            if del.abs() < sum.abs() * 1e-16 {
                break;
            }
        }
        (1.0 - sum * front).clamp(0.0, 1.0)
    } else {
        // Lentz's continued fraction for Q(a, x).
        const TINY: f64 = 1e-300;
        let mut b = x + 1.0 - a;
        let mut c = 1.0 / TINY;
        let mut d = 1.0 / b;
        let mut h = d;
        for i in 1..10_000 {
            let an = -(i as f64) * (i as f64 - a);
            b += 2.0;
            d = an * d + b;
            if d.abs() < TINY {
                d = TINY;
            }
            c = b + an / c;
            if c.abs() < TINY {
                c = TINY;
            }
            d = 1.0 / d;
            let del = d * c;
            h *= del;
            if (del - 1.0).abs() < 1e-16 {
                break;
            }
        }
        (front * h).clamp(0.0, 1.0)
    }
}

/// P(X >= `statistic`) for X chi-square with `dof` degrees of freedom; 1 at
/// no degrees of freedom (one category: nothing to differ).
pub fn chi_square_upper(statistic: f64, dof: usize) -> f64 {
    if dof == 0 {
        return 1.0;
    }
    if statistic.is_infinite() {
        return 0.0;
    }
    gamma_upper(dof as f64 / 2.0, statistic / 2.0)
}

/// What a discrete metric is judged by: the statistic, its degrees of
/// freedom and the chance of one at least as large from one distribution.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ChiSquare {
    pub statistic: f64,
    pub dof: usize,
    /// P(X >= statistic).
    pub p: f64,
}

/// Goodness of fit: `observed` counts per category against each category's
/// `probabilities` (summing to 1). A count in a category of probability 0 is
/// a value the rule never gives, so the statistic is infinite and p is 0.
pub fn chi_square_fit(observed: &[u64], probabilities: &[f64]) -> Result<ChiSquare, MetricError> {
    if observed.len() != probabilities.len() {
        return Err(MetricError::Categories {
            expected: probabilities.len(),
            found: observed.len(),
        });
    }
    if let Some((index, &value)) = probabilities
        .iter()
        .enumerate()
        .find(|(_, p)| !(p.is_finite() && **p >= 0.0))
    {
        return Err(MetricError::BadProbability { index, value });
    }
    let total_p: f64 = probabilities.iter().sum();
    if (total_p - 1.0).abs() > 1e-9 {
        return Err(MetricError::BadProbability {
            index: probabilities.len(),
            value: total_p,
        });
    }
    let n: u64 = observed.iter().sum();
    if n == 0 {
        return Err(MetricError::NoSamples);
    }
    let mut statistic = 0.0;
    let mut categories: usize = 0;
    for (&o, &p) in observed.iter().zip(probabilities) {
        if p == 0.0 {
            if o > 0 {
                statistic = f64::INFINITY;
            }
            continue;
        }
        categories += 1;
        let e = n as f64 * p;
        statistic += (o as f64 - e).powi(2) / e;
    }
    let dof = categories.saturating_sub(1);
    Ok(ChiSquare {
        statistic,
        dof,
        p: chi_square_upper(statistic, dof),
    })
}

/// Two samples' counts over the same categories, as a 2 x k contingency
/// table: whether they come from one distribution. Categories neither
/// sample holds are dropped.
pub fn chi_square_two(a: &[u64], b: &[u64]) -> Result<ChiSquare, MetricError> {
    if a.len() != b.len() {
        return Err(MetricError::Categories {
            expected: a.len(),
            found: b.len(),
        });
    }
    let (na, nb): (u64, u64) = (a.iter().sum(), b.iter().sum());
    if na == 0 || nb == 0 {
        return Err(MetricError::NoSamples);
    }
    let n = (na + nb) as f64;
    let mut statistic = 0.0;
    let mut categories: usize = 0;
    for (&x, &y) in a.iter().zip(b) {
        let column = (x + y) as f64;
        if column == 0.0 {
            continue;
        }
        categories += 1;
        let ex = na as f64 * column / n;
        let ey = nb as f64 * column / n;
        statistic += (x as f64 - ex).powi(2) / ex + (y as f64 - ey).powi(2) / ey;
    }
    let dof = categories.saturating_sub(1);
    Ok(ChiSquare {
        statistic,
        dof,
        p: chi_square_upper(statistic, dof),
    })
}

/// Discrete samples counted per distinct value, the same categories for both
/// (their union, in order).
pub fn counts(a: &[f64], b: &[f64]) -> (Vec<f64>, Vec<u64>, Vec<u64>) {
    let key = |v: f64| {
        if v == 0.0 {
            0.0f64.to_bits()
        } else {
            v.to_bits()
        }
    };
    let mut values: BTreeMap<u64, f64> = BTreeMap::new();
    for &v in a.iter().chain(b) {
        values.insert(key(v), v);
    }
    let mut order: Vec<f64> = values.into_values().collect();
    order.sort_by(f64::total_cmp);
    let count = |s: &[f64]| -> Vec<u64> {
        order
            .iter()
            .map(|&v| s.iter().filter(|&&x| key(x) == key(v)).count() as u64)
            .collect()
    };
    let (ca, cb) = (count(a), count(b));
    (order, ca, cb)
}

/// A list of metrics judged at one family-wise level: each fails when its p
/// is at or under `alpha / metrics` (Bonferroni).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Family {
    pub alpha: f64,
    pub metrics: usize,
}

impl Family {
    pub fn new(alpha: f64, metrics: usize) -> Result<Self, MetricError> {
        if !(alpha > 0.0 && alpha < 1.0) {
            return Err(MetricError::BadProbability {
                index: 0,
                value: alpha,
            });
        }
        if metrics == 0 {
            return Err(MetricError::NoSamples);
        }
        Ok(Self { alpha, metrics })
    }

    /// The family's alpha split evenly over its metrics (Bonferroni).
    pub fn per_metric(&self) -> f64 {
        self.alpha / self.metrics as f64
    }

    /// The two-sided |z| at which a rank-sum fails.
    pub fn z(&self) -> f64 {
        normal_quantile(1.0 - self.per_metric() / 2.0).expect("per_metric is in (0, 1)")
    }

    pub fn passes(&self, p: f64) -> bool {
        p > self.per_metric()
    }
}

/// How a metric's samples are compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Test {
    /// Continuous values, by the rank-sum.
    RankSum,
    /// A handful of discrete values, by chi-square over the values seen.
    ChiSquare,
}

/// One test's outcome: the rank-sum's z or chi-square's statistic, and p.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    pub statistic: f64,
    pub p: f64,
}

impl Test {
    /// `candidate` against `reference`. Both sides need a sample; samples
    /// that are all one value on both sides don't differ (z 0, p 1).
    pub fn run(self, reference: &[f64], candidate: &[f64]) -> Result<Outcome, MetricError> {
        if reference.is_empty() || candidate.is_empty() {
            return Err(MetricError::NoSamples);
        }
        if let Some(index) = reference
            .iter()
            .chain(candidate)
            .position(|v| !v.is_finite())
        {
            return Err(MetricError::NotFinite {
                offset: index as u64,
                component: 0,
            });
        }
        match self {
            Test::RankSum => Ok(match rank_sum_z(reference, candidate) {
                Some(z) => Outcome {
                    statistic: z,
                    p: two_sided_p(z),
                },
                None => Outcome {
                    statistic: 0.0,
                    p: 1.0,
                },
            }),
            Test::ChiSquare => {
                let (_, a, b) = counts(reference, candidate);
                let c = chi_square_two(&a, &b)?;
                Ok(Outcome {
                    statistic: c.statistic,
                    p: c.p,
                })
            }
        }
    }
}

/// One metric's reference: its samples from each oracle replay (a replay
/// gives one value for a total, several for a per-event metric).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Distribution {
    pub name: String,
    pub test: Test,
    replays: Vec<Vec<f64>>,
}

/// A metric judged: its outcome and whether it passed the family's level.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Judgement {
    pub name: String,
    pub outcome: Outcome,
    pub passed: bool,
}

impl Distribution {
    /// Needs a replay and a sample, every value finite.
    pub fn new(
        name: impl Into<String>,
        test: Test,
        replays: Vec<Vec<f64>>,
    ) -> Result<Self, MetricError> {
        if replays.is_empty() {
            return Err(MetricError::NoReplays);
        }
        if replays.iter().all(Vec::is_empty) {
            return Err(MetricError::NoSamples);
        }
        for (replay, values) in replays.iter().enumerate() {
            if let Some(component) = values.iter().position(|v| !v.is_finite()) {
                return Err(MetricError::NotFinite {
                    offset: replay as u64,
                    component,
                });
            }
        }
        Ok(Self {
            name: name.into(),
            test,
            replays,
        })
    }

    pub fn replays(&self) -> &[Vec<f64>] {
        &self.replays
    }

    /// The reference a candidate is judged against: one replay's handful of
    /// samples is too few, so the replays are pooled.
    pub fn pooled(&self) -> Vec<f64> {
        self.replays.iter().flatten().copied().collect()
    }

    pub fn judge(&self, candidate: &[f64], family: &Family) -> Result<Judgement, MetricError> {
        let outcome = self.test.run(&self.pooled(), candidate)?;
        Ok(Judgement {
            name: self.name.clone(),
            outcome,
            passed: family.passes(outcome.p),
        })
    }

    /// Each replay's samples judged against the other replays' pooled, at
    /// the family's level: a reference that fails its own replays is too
    /// small or holds a branch only some take. Needs three replays.
    pub fn leave_one_out(&self, family: &Family) -> Result<Vec<Judgement>, MetricError> {
        if self.replays.len() < 3 {
            return Err(MetricError::TooFewReplays {
                needed: 3,
                found: self.replays.len(),
            });
        }
        (0..self.replays.len())
            .map(|i| {
                let others: Vec<f64> = self
                    .replays
                    .iter()
                    .enumerate()
                    .filter(|&(j, _)| j != i)
                    .flat_map(|(_, r)| r.iter().copied())
                    .collect();
                let outcome = self.test.run(&others, &self.replays[i])?;
                Ok(Judgement {
                    name: format!("{} (replay {i} left out)", self.name),
                    outcome,
                    passed: family.passes(outcome.p),
                })
            })
            .collect()
    }
}

/// A list of metrics judged together, the family sized by the list.
pub fn judge_all(
    metrics: &[(&Distribution, &[f64])],
    alpha: f64,
) -> Result<Vec<Judgement>, MetricError> {
    let family = Family::new(alpha, metrics.len())?;
    metrics
        .iter()
        .map(|(d, candidate)| d.judge(candidate, &family))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_share_ties_and_the_rank_sum_reads_them() {
        assert_eq!(ranks(&[3.0, 1.0, 3.0, 2.0]), [3.5, 1.0, 3.5, 2.0]);
        assert!(
            rank_sum_z(&[1.0, 2.0, 3.0], &[1.0, 2.0, 3.0])
                .unwrap()
                .abs()
                < 1e-12
        );
        assert!(rank_sum_z(&[1.0, 2.0], &[3.0, 4.0]).unwrap() > 0.0);
        assert!(rank_sum_z(&[3.0, 4.0], &[1.0, 2.0]).unwrap() < 0.0);
        assert_eq!(rank_sum_z(&[1.0], &[1.0]), None);
        assert_eq!(rank_sum_z(&[], &[1.0]), None);
    }

    #[test]
    fn the_tails_match_their_tables() {
        // Normal: the familiar two-sided 5 % point and its Bonferroni five.
        assert!((normal_upper(1.959_963_985) - 0.025).abs() < 1e-8);
        assert!((two_sided_p(-1.959_963_985) - 0.05).abs() < 2e-8);
        assert!((normal_upper(0.0) - 0.5).abs() < 1e-9);
        assert!((normal_quantile(0.975).unwrap() - 1.959_964).abs() < 1e-6);
        assert!((normal_quantile(0.995).unwrap() - 2.575_829).abs() < 1e-6);
        assert!((normal_quantile(0.025).unwrap() + 1.959_964).abs() < 1e-6);
        for p in [0.0, 1.0, -0.5, f64::NAN] {
            assert!(normal_quantile(p).is_err());
        }
        // Chi-square: two degrees of freedom is exp(-x / 2) exactly, and the
        // 5 % points at 1, 3 and 5.
        for x in [0.5, 2.0, 9.0, 30.0] {
            assert!(
                (chi_square_upper(x, 2) - (-x / 2.0).exp()).abs() < 1e-12,
                "{x}"
            );
        }
        assert!((chi_square_upper(3.841_458_821, 1) - 0.05).abs() < 1e-8);
        assert!((chi_square_upper(7.814_727_903, 3) - 0.05).abs() < 1e-8);
        assert!((chi_square_upper(11.070_497_69, 5) - 0.05).abs() < 1e-8);
        assert_eq!(chi_square_upper(0.0, 3), 1.0);
        assert_eq!(chi_square_upper(5.0, 0), 1.0);
        assert_eq!(chi_square_upper(f64::INFINITY, 3), 0.0);
    }

    /// Ten values 1 to 10 against ten whose U is chosen: five above every
    /// one (50) and five beating `k` each.
    fn shifted(k: f64) -> (Vec<f64>, Vec<f64>) {
        let a: Vec<f64> = (1..=10).map(f64::from).collect();
        let mut b = vec![20.0; 5];
        b.extend([5.5, 5.5, 5.5, 5.5, k]);
        (a, b)
    }

    #[test]
    fn the_rank_sum_at_its_threshold() {
        let one = Family::new(0.05, 1).unwrap();
        assert!((one.z() - 1.959_964).abs() < 1e-6);
        // U = 75: z = 25 / sqrt(175) = 1.8898, inside 1.96.
        let (a, b) = shifted(5.5);
        let z = rank_sum_z(&a, &b).unwrap();
        assert!((z - 25.0 / 175f64.sqrt()).abs() < 1e-12);
        let pass = Test::RankSum.run(&a, &b).unwrap();
        assert!(one.passes(pass.p), "{pass:?}");
        // U = 76 (one value beating six): z = 1.9654, past it.
        let (a, b) = shifted(6.5);
        let fail = Test::RankSum.run(&a, &b).unwrap();
        assert!((fail.statistic - 26.0 / 175f64.sqrt()).abs() < 1e-12);
        assert!(!one.passes(fail.p), "{fail:?}");
        // Bonferroni over five metrics at 0.05: |z| 2.5758, so 1.9654 and
        // 2.3 pass where they'd fail alone.
        let five = Family::new(0.05, 5).unwrap();
        assert!((five.z() - 2.575_829).abs() < 1e-6);
        assert!(five.passes(fail.p));
        assert!(five.passes(two_sided_p(2.3)) && !one.passes(two_sided_p(2.3)));
        assert!(!five.passes(two_sided_p(2.6)));
        assert!(Family::new(0.0, 5).is_err() && Family::new(0.05, 0).is_err());
    }

    #[test]
    fn chi_square_at_its_threshold() {
        // 400 bites over 11-14, each a quarter: 7.84 is past the 5 % point
        // (7.8147 at 3 degrees of freedom), 7.30 inside it.
        let quarter = [0.25; 4];
        let fail = chi_square_fit(&[114, 86, 114, 86], &quarter).unwrap();
        assert!((fail.statistic - 7.84).abs() < 1e-12);
        assert_eq!(fail.dof, 3);
        assert!(fail.p < 0.05 && fail.p > 0.049, "{fail:?}");
        let pass = chi_square_fit(&[114, 86, 113, 87], &quarter).unwrap();
        assert!((pass.statistic - 7.30).abs() < 1e-12);
        assert!(pass.p > 0.05, "{pass:?}");
        let one = Family::new(0.05, 1).unwrap();
        assert!(!one.passes(fail.p) && one.passes(pass.p));
        // A value the rule never gives (a 15) fails outright.
        let never = chi_square_fit(&[100, 100, 100, 99, 1], &[0.25, 0.25, 0.25, 0.25, 0.0]);
        assert_eq!(never.unwrap().p, 0.0);
        assert!(chi_square_fit(&[1, 2], &[0.5, 0.25]).is_err());
        assert!(chi_square_fit(&[1, 2, 3], &[0.5, 0.5]).is_err());
        assert!(chi_square_fit(&[0, 0], &[0.5, 0.5]).is_err());
        // Two samples, by hand: 50/50 against 30/70 is 8.333 on 1.
        let two = chi_square_two(&[50, 50], &[30, 70]).unwrap();
        assert!((two.statistic - 25.0 / 3.0).abs() < 1e-12);
        assert_eq!(two.dof, 1);
        assert!((two.p - chi_square_upper(25.0 / 3.0, 1)).abs() < 1e-15);
        // An empty category drops out of the degrees of freedom.
        assert_eq!(chi_square_two(&[5, 0, 5], &[5, 0, 5]).unwrap().dof, 1);
        // Through `Test`: the values' union becomes the categories.
        let bites_a = [11.0, 12.0, 13.0, 14.0, 11.0, 12.0];
        let same = Test::ChiSquare.run(&bites_a, &bites_a).unwrap();
        assert_eq!((same.statistic, same.p), (0.0, 1.0));
        let (values, ca, cb) = counts(&[11.0, 12.0], &[12.0, 15.0, 12.0]);
        assert_eq!(
            (values, ca, cb),
            (vec![11.0, 12.0, 15.0], vec![1, 1, 0], vec![0, 2, 1])
        );
    }

    #[test]
    fn a_reference_judges_a_candidate_and_its_own_replays() {
        // Ten replays of a wait drawn evenly from 0 to 0.5 s, five samples
        // each, spread over the range.
        let replays: Vec<Vec<f64>> = (0..10)
            .map(|r| (0..5).map(|i| f64::from(i * 10 + r) / 100.0).collect())
            .collect();
        let d = Distribution::new("alert wait", Test::RankSum, replays).unwrap();
        let family = Family::new(0.05, 1).unwrap();
        for j in d.leave_one_out(&family).unwrap() {
            assert!(j.passed, "{j:?}");
        }
        // A candidate drawn the same way passes; one 0.2 s later fails.
        let inside: Vec<f64> = (0..50).map(|i| f64::from(i) / 100.0).collect();
        assert!(d.judge(&inside, &family).unwrap().passed);
        let late: Vec<f64> = inside.iter().map(|v| v + 0.2).collect();
        assert!(!d.judge(&late, &family).unwrap().passed);
        // A replay off on its own fails its leave-one-out.
        let mut off: Vec<Vec<f64>> = d.replays().to_vec();
        off[3] = vec![0.9, 0.95, 1.0, 1.05, 1.1];
        let off = Distribution::new("alert wait", Test::RankSum, off).unwrap();
        let verdicts = off.leave_one_out(&family).unwrap();
        assert!(!verdicts[3].passed);
        assert_eq!(verdicts.iter().filter(|j| !j.passed).count(), 1);
        // Judged against the others alone, never a reference holding itself.
        let others: Vec<f64> = off
            .replays()
            .iter()
            .enumerate()
            .filter(|&(j, _)| j != 3)
            .flat_map(|(_, r)| r.iter().copied())
            .collect();
        let z = rank_sum_z(&others, &off.replays()[3]).unwrap();
        assert_eq!(verdicts[3].outcome.statistic, z);
        // The list's size sets the level.
        let all = judge_all(&[(&d, &inside), (&d, &late)], 0.05).unwrap();
        assert!(all[0].passed && !all[1].passed);
        assert!(Distribution::new("x", Test::RankSum, vec![]).is_err());
        assert!(Distribution::new("x", Test::RankSum, vec![vec![], vec![]]).is_err());
        let two = Distribution::new("x", Test::RankSum, vec![vec![1.0], vec![2.0]]).unwrap();
        assert!(two.leave_one_out(&family).is_err());
        assert!(d.judge(&[], &family).is_err());
    }
}
