//! The paired statistics §9.4 asks for, and nothing else.
//!
//! Ticket 07 commits these numbers so ticket 08 is a comparison and not a
//! retrain: win rate with a Wilson interval, mean lines with a paired bootstrap
//! interval, and Evaluations per Solve as a median/p90 pair. Every definition is
//! pinned here so both tickets use one implementation.
//!
//! The **decision** does not use a float p-value: [`crate::width::mcnemar_rejects`]
//! is the exact integer two-sided sign test that §9.5 already ships at
//! `α = 0.01`. [`mcnemar_p`] exists so the record can *report* a p-value in the
//! rejection reason, and its value is not what decides anything.

use crate::width::{mcnemar_rejects, Paired};

/// One table's summary over the tuning Split.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub wins: u32,
    pub games: u32,
    pub win_rate: f64,
    /// Wilson 95 % interval, `z = 1.959963984540054`.
    pub win_rate_ci95: [f64; 2],
    /// Mean Lines over **all** games, so a loss counts as its Lines.
    pub mean_lines: f64,
    /// Mean Lines over the won games only: on a win the Solve stops at the Goal,
    /// so this is the Goal and says nothing — recorded so a reader can see that.
    pub mean_lines_on_wins: f64,
    /// Pieces the Sequence had to spare on a won game, averaged over wins; 0 on
    /// a loss. The trainer's third fitness term.
    pub mean_pieces_saved: f64,
    pub evals_mean: f64,
    pub evals_median: f64,
    pub evals_p90: f64,
    pub ms_median: f64,
    pub ms_p90: f64,
}

impl Stats {
    /// §9.1's `≥ 99 %`: the axis this bed cannot rank on, named rather than
    /// assumed.
    pub fn saturated(&self) -> bool {
        self.games > 0 && (self.wins as u64) * 100 >= (self.games as u64) * 99
    }
}

/// One table's per-seed outcome, the unit the pairing is built from.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    pub seed: u32,
    pub won: bool,
    pub lines: u32,
    pub evals: u32,
    pub moves: u32,
    pub ms: f64,
    pub pieces: u32,
}

impl Sample {
    pub fn pieces_saved(&self) -> f64 {
        if self.won && self.pieces > self.moves {
            (self.pieces - self.moves) as f64
        } else {
            0.0
        }
    }
}

/// Summarise one table over its samples. The samples must be in seed order; the
/// sums are accumulated in that order so the last bit is fixed (notes/07 §7.5).
pub fn summarise(samples: &[Sample]) -> Stats {
    let games = samples.len() as u32;
    if games == 0 {
        return Stats::default();
    }
    let wins = samples.iter().filter(|sample| sample.won).count() as u32;
    let mut lines_sum = 0.0;
    let mut lines_on_wins = 0.0;
    let mut saved_sum = 0.0;
    let mut evals_sum = 0.0;
    for sample in samples {
        lines_sum += sample.lines as f64;
        evals_sum += sample.evals as f64;
        if sample.won {
            lines_on_wins += sample.lines as f64;
            saved_sum += sample.pieces_saved();
        }
    }
    let win_rate = wins as f64 / games as f64;
    let mut evals: Vec<f64> = samples.iter().map(|sample| sample.evals as f64).collect();
    let mut ms: Vec<f64> = samples.iter().map(|sample| sample.ms).collect();
    evals.sort_by(|a, b| a.partial_cmp(b).expect("evaluations are finite"));
    ms.sort_by(|a, b| a.partial_cmp(b).expect("milliseconds are finite"));
    Stats {
        wins,
        games,
        win_rate,
        win_rate_ci95: wilson(wins, games),
        mean_lines: lines_sum / games as f64,
        mean_lines_on_wins: if wins > 0 {
            lines_on_wins / wins as f64
        } else {
            0.0
        },
        mean_pieces_saved: if wins > 0 {
            saved_sum / wins as f64
        } else {
            0.0
        },
        evals_mean: evals_sum / games as f64,
        evals_median: median(&evals),
        evals_p90: p90(&evals),
        ms_median: median(&ms),
        ms_p90: p90(&ms),
    }
}

/// Wilson's score interval at 95 %. A plain `p ± 1.96·√(p(1−p)/n)` gives a
/// zero-width interval at 100 %, which is exactly where a saturated bed sits.
pub fn wilson(wins: u32, games: u32) -> [f64; 2] {
    if games == 0 {
        return [0.0, 0.0];
    }
    const Z: f64 = 1.959_963_984_540_054;
    let n = games as f64;
    let p = wins as f64 / n;
    let z2 = Z * Z;
    let denominator = 1.0 + z2 / n;
    let centre = p + z2 / (2.0 * n);
    let spread = Z * ((p * (1.0 - p) / n + z2 / (4.0 * n * n)).max(0.0)).sqrt();
    [
        ((centre - spread) / denominator).clamp(0.0, 1.0),
        ((centre + spread) / denominator).clamp(0.0, 1.0),
    ]
}

/// Nearest-rank p90: `ceil(0.9·n)`, on a sorted slice (notes/07 §5.2).
pub fn p90(sorted: &[f64]) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((sorted.len() as f64) * 0.9).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

pub fn median(sorted: &[f64]) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    }
}

/// The paired discordant counts with `reference` = E1 and `judged` = E2:
/// `b` = E1 wins where E2 loses, `c` = E2 wins where E1 loses.
///
/// The pairing is exact because both tables played the same seed, hence the same
/// Board and the same Sequence (§5.7).
pub fn paired(reference: &[Sample], judged: &[Sample]) -> Paired {
    assert_eq!(
        reference.len(),
        judged.len(),
        "a pairing needs equal lengths"
    );
    let mut counts = Paired::default();
    for (a, b) in reference.iter().zip(judged) {
        assert_eq!(a.seed, b.seed, "the rows are not paired by seed");
        match (a.won, b.won) {
            (true, false) => counts.b += 1,
            (false, true) => counts.c += 1,
            _ => {}
        }
    }
    counts
}

/// §9.4's ship rule is ticket 08's; this is only 07's pre-registered *rejection*
/// rule (notes/07 §6.1), which is about "worse", never about "better".
pub fn worse_than_published(
    reference: &[Sample],
    judged: &[Sample],
    ci95: [f64; 2],
) -> Option<&'static str> {
    let discordant = paired(reference, judged);
    let e1_wins = reference.iter().filter(|sample| sample.won).count();
    let e2_wins = judged.iter().filter(|sample| sample.won).count();
    // 1. Win rate: E1 higher, and the exact sign test rejects at α = 0.01.
    if e1_wins > e2_wins && mcnemar_rejects(discordant) {
        return Some("win_rate");
    }
    // 2. Mean lines, only when the win rates tie: the paired bootstrap interval
    //    of `lines_E2 − lines_E1` lies entirely below zero.
    if e1_wins == e2_wins && ci95[1] < 0.0 {
        return Some("mean_lines");
    }
    // 3. Evaluations per solve is §9.4's tie-break, not a significance test: a
    //    table is never "worse" for using more evaluations, it just loses a tie.
    None
}

/// An exact-ish two-sided McNemar p-value, for the record's prose.
///
/// The **decision** is [`mcnemar_rejects`]; this number exists so the rejection
/// reason can quote one. `n ≤ 1,000` is computed exactly in log space; above that
/// the normal approximation with a continuity correction is used, which cannot
/// happen on a 1,000-game pairing.
pub fn mcnemar_p(discordant: Paired) -> f64 {
    let n = discordant.n();
    if n == 0 {
        return 1.0;
    }
    let m = discordant.b.min(discordant.c);
    if n > 1_000 {
        let z = ((discordant.b as f64 - discordant.c as f64).abs() - 1.0) / (n as f64).sqrt();
        return 2.0 * (1.0 - normal_cdf(z));
    }
    // p = 2 · 2^-n · Σ_{k≤m} C(n,k), accumulated as logs so no term overflows.
    let mut sum = 0.0f64;
    let mut log_term = 0.0f64; // log C(n, 0)
    for k in 0..=m {
        if k > 0 {
            log_term += ((n - k + 1) as f64).ln() - (k as f64).ln();
        }
        sum += log_term.exp();
    }
    let p = 2.0 * sum * (-(n as f64) * std::f64::consts::LN_2).exp();
    p.clamp(0.0, 1.0)
}

fn normal_cdf(z: f64) -> f64 {
    // Abramowitz & Stegun 7.1.26 on erf; four decimal places is far more than a
    // reported p-value needs and it uses no table.
    let x = z.abs() / std::f64::consts::SQRT_2;
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let erf = 1.0
        - (((((1.061_405_429 * t - 1.453_152_027) * t) + 1.421_413_741) * t - 0.284_496_736) * t
            + 0.254_829_592)
            * t
            * (-x * x).exp();
    0.5 * (1.0 + if z >= 0.0 { erf } else { -erf })
}

/// A paired bootstrap 95 % interval for the mean of `judged − reference`, over
/// the seed indices.
///
/// 10,000 resamples with replacement, drawn from splitmix64 seeded by the run's
/// committed `tune_seed ^ C_BOOTSTRAP`, so the interval is a function of the
/// record's own numbers. Percentile 2.5/97.5 by nearest rank.
///
/// Mean lines is the statistic §9.4 pairs with the exact McNemar test. Ticket 08
/// briefly needed the same interval on Evaluations per Solve, to judge Dedup on the
/// axis Dedup claimed; the ladder then deleted Dedup (§9.4), so the cost axis went
/// with it and this is once again the one interval this crate computes.
pub fn paired_bootstrap_ci95(
    reference: &[Sample],
    judged: &[Sample],
    tune_seed: u64,
    resamples: u32,
) -> [f64; 2] {
    interval_of(reference, judged, tune_seed, resamples, |sample| {
        sample.lines as i64
    })
}

/// The one implementation: a paired percentile interval of one integer per seed.
fn interval_of(
    reference: &[Sample],
    judged: &[Sample],
    tune_seed: u64,
    resamples: u32,
    value: impl Fn(&Sample) -> i64,
) -> [f64; 2] {
    assert_eq!(reference.len(), judged.len());
    let n = reference.len();
    if n == 0 || resamples == 0 {
        return [0.0, 0.0];
    }
    let deltas: Vec<f64> = reference
        .iter()
        .zip(judged)
        .map(|(a, b)| value(b) as f64 - value(a) as f64)
        .collect();
    let mut rng = super::rng::SplitMix64::bootstrap_stream(tune_seed);
    let mut means: Vec<f64> = Vec::with_capacity(resamples as usize);
    for _ in 0..resamples {
        let mut sum = 0.0f64;
        for _ in 0..n {
            let index = rng.bounded(n as u32) as usize;
            sum += deltas[index];
        }
        means.push(sum / n as f64);
    }
    means.sort_by(|a, b| a.partial_cmp(b).expect("deltas are finite"));
    [percentile(&means, 0.025), percentile(&means, 0.975)]
}

/// Percentile by nearest rank on a sorted slice.
fn percentile(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (q * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// FNV-1a-64 over the eight `i32` as little-endian bytes: the table's identity.
///
/// One number that says "this is that vector", so `bench tune --replay` can
/// compare a re-run against the committed table without comparing prose.
pub fn table_fnv(table: &[i32; 8]) -> String {
    let mut fnv = crate::fingerprint::Fnv::new();
    for weight in table {
        fnv.u32_le(*weight as u32);
    }
    format!("0x{}", crate::fingerprint::hex(fnv.finish()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wilson_is_not_degenerate_at_the_edges() {
        let [low, high] = wilson(1_000, 1_000);
        assert!(low > 0.99 && low < 1.0, "{low}");
        assert_eq!(high, 1.0);
        let [low, high] = wilson(0, 1_000);
        assert!((0.0..1e-9).contains(&low), "{low}");
        assert!(high > 0.0 && high < 0.01, "{high}");
    }

    #[test]
    fn p90_is_nearest_rank() {
        let samples: Vec<f64> = (1..=10).map(|n| n as f64).collect();
        assert_eq!(p90(&samples), 9.0);
        let samples: Vec<f64> = (1..=100).map(|n| n as f64).collect();
        assert_eq!(p90(&samples), 90.0);
    }

    #[test]
    fn the_exact_sign_test_and_the_reported_p_agree_on_the_threshold() {
        // n = 8, m = 0: p = 2/256 = 0.0078 < 0.01, so it rejects.
        let eight = Paired { b: 8, c: 0 };
        assert!(mcnemar_rejects(eight));
        assert!(mcnemar_p(eight) < 0.01);
        // n = 7, m = 0: p = 2/128 = 0.0156 > 0.01, so it does not.
        let seven = Paired { b: 7, c: 0 };
        assert!(!mcnemar_rejects(seven));
        assert!(mcnemar_p(seven) > 0.01);
    }

    #[test]
    fn a_paired_bootstrap_interval_brackets_zero_for_a_tie() {
        let reference: Vec<Sample> = (0..100)
            .map(|seed| Sample {
                seed,
                won: true,
                lines: 12,
                ..Sample::default()
            })
            .collect();
        let judged = reference.clone();
        let [low, high] = paired_bootstrap_ci95(&reference, &judged, 5, 1_000);
        assert_eq!([low, high], [0.0, 0.0]);
    }

    #[test]
    fn the_rejection_rule_needs_a_significant_loss() {
        let reference: Vec<Sample> = (0..100)
            .map(|seed| Sample {
                seed,
                won: true,
                lines: 12,
                ..Sample::default()
            })
            .collect();
        let mut judged = reference.clone();
        for sample in judged.iter_mut().take(3) {
            sample.won = false;
            sample.lines = 9;
        }
        assert_eq!(worse_than_published(&reference, &judged, [0.0, 0.0]), None);
        let mut judged = reference.clone();
        for sample in judged.iter_mut().take(12) {
            sample.won = false;
            sample.lines = 9;
        }
        assert_eq!(
            worse_than_published(&reference, &judged, [0.0, 0.0]),
            Some("win_rate")
        );
    }

    #[test]
    fn the_table_fingerprint_is_stable() {
        assert_eq!(table_fnv(&[0; 8]), "0x0c8210784d8af5a5");
    }
}
