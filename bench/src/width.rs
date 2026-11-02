//! The width curve: `DEFAULT_BEAM` and the Width cap (§9.5), measured once and
//! committed as an artifact the page reads at build time.
//!
//! Two questions, two axes, one file:
//!
//! * **Cost.** Across the doubling points 25 … 6,400, how long does a Solve take?
//!   The **Width cap** is the largest measured width at which a default
//!   configuration's Solve stays at p90 ≤ [`DEFAULT_BUDGET_MS`] *and* a harder
//!   configuration's at p90 ≤ [`HARDER_BUDGET_MS`] (§9.3 is where those two
//!   numbers live, and 200 ms is both the budget and §9.5's cap criterion).
//! * **Quality.** Per seed, at each doubling point, did the game win? The
//!   **default width** is the narrowest doubling point whose win rate on the
//!   ranking bed is a *tie* with the cap's — a tie being an exact McNemar
//!   `p ≥ 0.01` **and** a shortfall of at most 1.0 pp, with ties resolving
//!   narrower.
//!
//! §9.1's saturation rule is checked first, at the cap: a bed that wins ≥ 99%
//! cannot rank anything at any width, so when either shipped configuration
//! saturates, the quality axis moves to the validation-only bed (goal 16, 160
//! pieces, cap 7, seeds 1,000–1,999 — its own test split, which keeps seeds
//! 0–999 pristine for the Reference run). If that bed saturates too, the artifact
//! says the quality axis had no resolution and names the cost-chosen width as
//! such: a cost decision must not be dressed up as a quality result.
//!
//! Everything in here is integer-exact except the milliseconds it reports. The
//! McNemar p-value is never stored as a float (`p` can be `2^-1999`): the
//! discordant counts `(b, c)` are the exact sufficient statistic and the
//! comparison `200 · Σ C(n,k) < 2^n` is done in a small big integer, so a reader
//! can recompute the decision years later without a statistics library.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use crate::dataset::{self, Config};
use crate::fingerprint::{self, Fnv};
use crate::json::{self, Value};

// ------------------------------------------------------------------ the record

/// §9.5's doubling points, ascending. The list is pre-registered: re-running the
/// curve may stop earlier, never pick different points.
pub const WIDTHS: [u32; 9] = [25, 50, 100, 200, 400, 800, 1_600, 3_200, 6_400];

/// §9.3: the default configuration's Solve budget. It is also §9.5's cap
/// criterion, which is why there is no second threshold anywhere.
pub const DEFAULT_BUDGET_MS: u64 = 200;

/// §9.3: the harder configuration's Solve budget.
pub const HARDER_BUDGET_MS: u64 = 1_000;

/// §9.3's two budgets, in `Bed::SHIPPED` order. The default configuration is the
/// binding constraint (§9.5's cap criterion is 200 ms); the harder one gets a
/// second. One array, so nothing in this file can disagree about which is which.
pub const BUDGETS_MS: [u64; 2] = [DEFAULT_BUDGET_MS, HARDER_BUDGET_MS];

/// §9.1: a configuration winning this percentage cannot rank anything, at any
/// width.
pub const SATURATION_PERCENT: u32 = 99;

/// §9.5's tie: an exact McNemar `p ≥ 0.01`, i.e. `100 · p ≥ 1`.
pub const ALPHA_INVERSE: u64 = 100;

/// §9.5's tie: a shortfall of at most 1.0 pp, in tenths of a percentage point.
pub const MAX_SHORTFALL_TENTHS_PP: i64 = 10;

/// The validation-only configuration of §9.1: never shipped, never in a Run, and
/// the ranking bed when the shipped beds saturate.
pub const VALIDATION_GOAL: u32 = 16;
pub const VALIDATION_PIECES: u32 = 160;
pub const VALIDATION_CAP: u32 = 7;

/// §6's splits. The shipped beds' test split, and the validation bed's own.
pub const TEST_SPLIT: (u32, u32) = dataset::TEST_SPLIT;
pub const TUNING_SPLIT: (u32, u32) = dataset::TUNING_SPLIT;

/// One measured configuration: the three numbers that name a bed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bed {
    pub goal: u32,
    pub pieces: u32,
    pub cap: u32,
}

impl Bed {
    pub const GOAL8: Bed = Bed {
        goal: 8,
        pieces: 40,
        cap: 7,
    };
    pub const GOAL12: Bed = Bed {
        goal: 12,
        pieces: 100,
        cap: 7,
    };
    pub const GOAL16: Bed = Bed {
        goal: VALIDATION_GOAL,
        pieces: VALIDATION_PIECES,
        cap: VALIDATION_CAP,
    };

    /// The two beds the page ships, in the order the artifact lists them: the
    /// default configuration first (it is the binding constraint).
    pub const SHIPPED: [Bed; 2] = [Bed::GOAL8, Bed::GOAL12];

    /// `goal8-p40-cap7`, the Dataset's directory name.
    pub fn name(&self) -> String {
        format!("goal{}-p{}-cap{}", self.goal, self.pieces, self.cap)
    }

    /// Whether this bed has a committed Dataset (the validation bed does not).
    pub fn is_shipped(&self) -> bool {
        Bed::SHIPPED.contains(self)
    }

    fn config(&self) -> Option<Config> {
        dataset::CONFIGS
            .iter()
            .copied()
            .find(|config| config.dir == self.name())
    }
}

/// A win rate as two integers. Never a float: §9.5's legs are compared exactly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub wins: u32,
    pub games: u32,
}

impl Tally {
    /// §9.1's `≥ 99%`, exactly.
    pub fn saturated(self) -> bool {
        self.games > 0
            && (self.wins as u64) * 100 >= (self.games as u64) * (SATURATION_PERCENT as u64)
    }
}

/// The paired discordant counts between two widths over the same seeds:
/// `b` = reference wins where the judged width loses, `c` = the other way round.
/// `b - c` is therefore exactly the win-count difference.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Paired {
    pub b: u32,
    pub c: u32,
}

impl Paired {
    pub fn n(self) -> u32 {
        self.b + self.c
    }
}

// ------------------------------------------------------------------ exact stats

/// A big unsigned integer, little-endian 32-bit limbs: just what an exact
/// two-sided sign test needs at `n ≤ 2,000`, i.e. `2^2000` and `C(2000, 1000)`.
/// `bench` has no dependencies by design (`run/log.md`), and a float p-value
/// underflows long before 1,000 paired games.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Big {
    limbs: Vec<u32>,
}

impl Big {
    fn zero() -> Big {
        Big { limbs: Vec::new() }
    }

    fn pow2(exponent: u32) -> Big {
        let mut limbs = vec![0u32; (exponent / 32) as usize + 1];
        limbs[(exponent / 32) as usize] = 1 << (exponent % 32);
        Big::normalised(limbs)
    }

    fn normalised(mut limbs: Vec<u32>) -> Big {
        while limbs.last() == Some(&0) {
            limbs.pop();
        }
        Big { limbs }
    }

    fn add_small(&mut self, value: u32) {
        let mut carry = value as u64;
        let mut index = 0;
        while carry != 0 {
            if index == self.limbs.len() {
                self.limbs.push(0);
            }
            let sum = self.limbs[index] as u64 + carry;
            self.limbs[index] = sum as u32;
            carry = sum >> 32;
            index += 1;
        }
    }

    fn add(&mut self, other: &Big) {
        if self.limbs.len() < other.limbs.len() {
            self.limbs.resize(other.limbs.len(), 0);
        }
        let mut carry = 0u64;
        for index in 0..self.limbs.len() {
            let rhs = other.limbs.get(index).copied().unwrap_or(0) as u64;
            let sum = self.limbs[index] as u64 + rhs + carry;
            self.limbs[index] = sum as u32;
            carry = sum >> 32;
        }
        if carry != 0 {
            self.limbs.push(carry as u32);
        }
    }

    fn mul_small(&mut self, factor: u64) {
        let mut carry = 0u128;
        for limb in &mut self.limbs {
            let product = (*limb as u128) * (factor as u128) + carry;
            *limb = product as u32;
            carry = product >> 32;
        }
        while carry != 0 {
            self.limbs.push(carry as u32);
            carry >>= 32;
        }
    }

    /// Exact division by a small number, returning the remainder.
    fn div_small(&mut self, divisor: u64) -> u64 {
        let mut remainder = 0u128;
        for limb in self.limbs.iter_mut().rev() {
            let current = (remainder << 32) | (*limb as u128);
            *limb = (current / divisor as u128) as u32;
            remainder = current % divisor as u128;
        }
        while self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
        remainder as u64
    }

    fn cmp_big(&self, other: &Big) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        if self.limbs.len() != other.limbs.len() {
            return self.limbs.len().cmp(&other.limbs.len());
        }
        for index in (0..self.limbs.len()).rev() {
            match self.limbs[index].cmp(&other.limbs[index]) {
                Ordering::Equal => continue,
                other_order => return other_order,
            }
        }
        Ordering::Equal
    }
}

/// §9.5's first leg, exactly: the two-sided exact McNemar test rejects at
/// `α = 0.01` when `200 · Σ_{k ≤ m} C(n,k) < 2^n`, `n = b + c`, `m = min(b, c)`.
///
/// The ordinary binomial sign test on the discordant pairs: it needs no
/// continuity correction and is calibrated at the small discordance a 1,000-game
/// pairing actually produces, where a χ² approximation is not.
pub fn mcnemar_rejects(paired: Paired) -> bool {
    let n = paired.n();
    if n == 0 {
        return false;
    }
    let m = paired.b.min(paired.c);

    // Σ_{k=0..m} C(n, k), built in place: C(n,k) = C(n,k-1) · (n-k+1) / k.
    let mut sum = Big::zero();
    let mut term = Big::zero();
    term.add_small(1); // C(n, 0) = 1
    sum.add(&term);
    for k in 1..=m {
        term.mul_small((n - k + 1) as u64);
        let remainder = term.div_small(k as u64);
        debug_assert_eq!(remainder, 0, "C(n,k) is an integer");
        sum.add(&term);
    }
    sum.mul_small(2 * ALPHA_INVERSE); // p < 1/100  <=>  200·Σ < 2^n

    let threshold = Big::pow2(n);
    matches!(sum.cmp_big(&threshold), std::cmp::Ordering::Less)
}

/// How many wins the judged width is behind the reference width. Negative when
/// it is ahead. Exact integer arithmetic: both tallies share a denominator in
/// every call this file makes, and the general form is a cross-multiplied
/// comparison.
pub fn shortfall_wins(judged: Tally, reference: Tally) -> i64 {
    if judged.games == 0 || reference.games == 0 {
        return 0;
    }
    if judged.games == reference.games {
        return reference.wins as i64 - judged.wins as i64;
    }
    // Expressed in wins on the reference's denominator, rounded towards zero.
    let left = reference.wins as i64 * judged.games as i64;
    let right = judged.wins as i64 * reference.games as i64;
    (left - right) / judged.games as i64
}

/// §9.5's second leg, exactly: `100 · shortfall ≤ games`, i.e. at 1,000 paired
/// games at most 10 wins behind.
pub fn shortfall_within_one_point(judged: Tally, reference: Tally) -> bool {
    let behind = shortfall_wins(judged, reference);
    if behind <= 0 {
        return true;
    }
    // In tenths of a percentage point, so every term stays an integer:
    // `behind / games <= 1.0 %`  <=>  `behind * 1000 <= games * 10`.
    behind * 1_000 <= reference.games as i64 * MAX_SHORTFALL_TENTHS_PP
}

/// §9.5's tie predicate: *both* legs. McNemar alone would let a width trailing the
/// cap by 2 pp ship as the default, because 1,000 paired games cannot resolve it.
pub fn tie(judged: Tally, reference: Tally, paired: Paired) -> bool {
    !mcnemar_rejects(paired) && shortfall_within_one_point(judged, reference)
}

/// One doubling step's paired counts, in the order the sweep measured them:
/// `from` the narrower width, `to` the wider one, `paired` = {reference wins and
/// judged loses, judged wins and reference loses} with the narrower width as the
/// reference.
pub type Step = (Tally, Tally, Paired);

/// A step is **flat** when the §9.5 tie predicate holds across it — the same
/// statistic, so the stop rule invents no second threshold.
pub fn flat(judged: Tally, reference: Tally, paired: Paired) -> bool {
    tie(judged, reference, paired)
}

/// How many flat steps the sweep ends on.
pub fn trailing_flat_steps(steps: &[Step]) -> usize {
    steps
        .iter()
        .rev()
        .take_while(|(to, from, paired)| flat(*to, *from, *paired))
        .count()
}

/// §9.5's stop rule: two consecutive flat steps, and then the climb is over.
pub fn sweep_is_flat(steps: &[Step]) -> bool {
    trailing_flat_steps(steps) >= 2
}

/// §9.1's resolution test for the bed that ranked the widths: `≥ 99%` at the cap
/// means the quality axis has no resolution at all.
pub fn quality_axis(cap: Tally) -> QualityAxis {
    if cap.saturated() {
        QualityAxis::NoResolution
    } else {
        QualityAxis::Resolved
    }
}

// ------------------------------------------------------------------ percentiles

/// The nearest-rank order statistic: the smallest value such that at least
/// `num/den` of the samples are ≤ it. Not interpolated — one integer index, so a
/// reader can recompute it from the sorted array, and a single slow outlier does
/// not get smeared across two neighbours.
pub fn nearest_rank(sorted: &[f64], num: usize, den: usize) -> f64 {
    assert!(
        !sorted.is_empty(),
        "a percentile of nothing is not a number"
    );
    let rank = (num * sorted.len()).div_ceil(den).max(1);
    sorted[rank - 1]
}

/// p90 of a sample, sorted in place by the caller's copy.
pub fn p90(samples: &[f64]) -> f64 {
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("no NaN in a timer"));
    nearest_rank(&sorted, 9, 10)
}

fn median(samples: &[f64]) -> f64 {
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("no NaN in a timer"));
    nearest_rank(&sorted, 1, 2)
}

// -------------------------------------------------------------- the Width cap

/// One width's cost row: the two p90s, in `Bed::SHIPPED` order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CostRow {
    pub width: u32,
    /// `[default configuration, harder configuration]`, in ms.
    pub p90_ms: [f64; 2],
}

impl CostRow {
    /// Both budgets hold at this width.
    pub fn fits(&self) -> bool {
        self.p90_ms[0] <= BUDGETS_MS[0] as f64 && self.p90_ms[1] <= BUDGETS_MS[1] as f64
    }

    /// One bed's budget, by the same index the row holds it at.
    fn budget(index: usize) -> u64 {
        BUDGETS_MS[index]
    }
}

/// What stopped the climb: a budget, or the end of the pre-registered list. The
/// tooltip must say which (§2.2), so it is not a comment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapBound {
    /// A width failed a budget — measured above the cap, or probed there.
    Budget,
    /// Every pre-registered width was measured and passed; the doubling list is
    /// what stopped it. **Only reachable for a complete curve**: a run that asked
    /// for a prefix of the list was stopped by its own request, and calling that
    /// the schedule would tell the page's tooltip the climb was still rising.
    Schedule,
}

/// §9.5: the largest measured width that fits both budgets. `rows` must be
/// ascending and must include the first failing width, or the word "largest" is
/// unsupported — which is why a curve that never fails is `Schedule`, not an
/// error, and a curve whose narrowest point already fails is one.
pub fn width_cap(rows: &[CostRow]) -> Result<(u32, CapBound), String> {
    if rows.is_empty() {
        return Err("no measured widths: there is no cap to name".to_string());
    }
    if rows
        .windows(2)
        .any(|window| window[0].width >= window[1].width)
    {
        return Err("cost rows must be ascending by width".to_string());
    }
    let mut cap: Option<u32> = None;
    let mut bound = CapBound::Schedule;
    for row in rows {
        if row.fits() {
            cap = Some(row.width);
        } else {
            if cap.is_none() {
                return Err(format!(
                    "width {} already fails a budget (p90 {:.1} ms / {:.1} ms): no width fits",
                    row.width, row.p90_ms[0], row.p90_ms[1]
                ));
            }
            bound = CapBound::Budget;
            break;
        }
    }
    cap.map(|width| (width, bound))
        .ok_or_else(|| "no measured width fits both budgets".to_string())
}

// -------------------------------------------------- the evidence a curve rests on

/// A width probed **outside** the curve, to confirm the stop rule instead of
/// guessing at it: one bed, one substrate, too few solves to be a doubling point.
///
/// This is evidence about *why* the climb stopped, and it is never a point. A point
/// is a width measured on both shipped beds with §9.5's trials and seeds; letting a
/// ten-seed probe into `points` would let it decide the cap, the default width and
/// the quality fingerprint, none of which it has the sample size to decide. So it
/// gets its own object in the artifact, and its one job is to make
/// `cap_bound: "budget"` checkable rather than asserted in prose.
#[derive(Clone, Debug, PartialEq)]
pub struct Overshoot {
    /// The probed width: a pre-registered doubling point above the cap.
    pub width: u32,
    /// The substrate the probe ran on (`native` or `wasm`). The curve's own cost
    /// series may be on the other one, which is what [`Overshoot::note`] is for.
    pub substrate: String,
    /// Solves behind [`Overshoot::ms_p90`] — a p90 without a sample size is not a
    /// measurement (§9.3).
    pub samples: u32,
    pub ms_p90: f64,
    /// The operator's sentence: what the probe showed, and how it extrapolates to
    /// the substrate the cost series is measured on. Prose, because it is a human's
    /// claim and the reader has to be able to see that it is one.
    pub note: String,
}

impl Overshoot {
    /// §9.5's cap criterion is the default configuration's budget, so a probe exists
    /// to show that one number was crossed.
    fn budget_ms(&self) -> u64 {
        BUDGETS_MS[0]
    }

    /// The bed whose budget the probe failed: the default configuration, the
    /// binding constraint.
    fn bed(&self) -> String {
        Bed::SHIPPED[0].name()
    }

    /// Evidence, or nothing: a probe is only evidence about the stop rule if it is
    /// a pre-registered width, above the cap, with a real sample size, over the
    /// budget it claims to fail, on a substrate it names.
    pub fn check(&self, cap: u32) -> Result<(), String> {
        if !WIDTHS.contains(&self.width) {
            return Err(format!(
                "the overshoot probe at width {} is not one of the pre-registered doubling points",
                self.width
            ));
        }
        if self.width <= cap {
            return Err(format!(
                "the overshoot probe at width {} is not above the cap {cap}: a width that fails a \
                 budget cannot sit at or below the largest width that fits both",
                self.width
            ));
        }
        if self.samples == 0 {
            return Err(format!(
                "the overshoot probe at width {} records no samples: a p90 without a sample size \
                 is not a measurement",
                self.width
            ));
        }
        if !self.ms_p90.is_finite() || self.ms_p90 <= self.budget_ms() as f64 {
            return Err(format!(
                "the overshoot probe at width {} reads p90 {:.2} ms against the {} ms budget: a \
                 probe that fits is not why the climb stopped",
                self.width,
                self.ms_p90,
                self.budget_ms()
            ));
        }
        if self.substrate.trim().is_empty() {
            return Err(format!(
                "the overshoot probe at width {} does not say which substrate it ran on",
                self.width
            ));
        }
        Ok(())
    }
}

/// The fields an artifact **derives** from its evidence rather than measures. One
/// place computes them, for a live run and for a re-derivation of a committed file,
/// so the two cannot disagree about what the curve did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub width_cap: u32,
    pub cap_bound: CapBound,
    pub complete: bool,
    pub missing_widths: Vec<u32>,
    pub incomplete_reason: String,
}

/// Derive the summary fields from the measured points and the probe, against the
/// **pre-registered** doubling list.
///
/// That distinction is the whole function. A run may legitimately ask for a prefix
/// of §9.5's nine points (`--widths 25,50,100,200,400,800` is how the committed
/// artifact was produced), so `complete` cannot mean "the widths I was asked for
/// all came back": it means "every pre-registered point is a measured point".
/// Likewise `cap_bound: schedule` is reachable only when that is true *and* every
/// point passed its budget, because the word drives a tooltip that says "the curve
/// was still climbing at the last measured width" — which is a lie about a curve
/// that won 1,000/1,000 sends everywhere it was measured. A truncated curve that
/// records no failing width and no probe has nothing to put there, so it is an
/// error: the honest answer is that the artifact cannot say what stopped the climb.
pub fn summarise(points: &[Point], overshoot: Option<&Overshoot>) -> Result<Summary, String> {
    let rows: Vec<CostRow> = points
        .iter()
        .map(|point| CostRow {
            width: point.width,
            p90_ms: [
                point.cost.first().map_or(f64::NAN, |bed| bed.ms_p90),
                point.cost.get(1).map_or(f64::NAN, |bed| bed.ms_p90),
            ],
        })
        .collect();
    // Checks the list is non-empty and ascending, and names the cap.
    let (width_cap, measured_bound) = width_cap(&rows)?;
    for point in points {
        if !WIDTHS.contains(&point.width) {
            return Err(format!(
                "width {} is not one of the pre-registered doubling points, so it cannot be part \
                 of the curve",
                point.width
            ));
        }
    }

    if let Some(probe) = overshoot {
        probe.check(width_cap)?;
        if points.iter().any(|point| point.width == probe.width) {
            return Err(format!(
                "width {} is recorded both as a measured point and as an overshoot probe: a probe \
                 is a width the curve did not measure",
                probe.width
            ));
        }
    }

    let missing_widths: Vec<u32> = WIDTHS
        .iter()
        .copied()
        .filter(|width| !points.iter().any(|point| point.width == *width))
        .collect();
    let probed = overshoot.filter(|probe| probe.width > width_cap).is_some();
    let cap_bound = if measured_bound == CapBound::Budget || probed {
        CapBound::Budget
    } else if missing_widths.is_empty() {
        CapBound::Schedule
    } else {
        return Err(format!(
            "widths {} were not measured and nothing on the record says why the climb stopped: \
             the pre-registered list does not end at {width_cap} and no probe failed a budget. \
             Measure the next point, or record the probe that failed it",
            list(&missing_widths)
        ));
    };

    let complete = missing_widths.is_empty();
    let incomplete_reason = if complete {
        String::new()
    } else {
        stop_reason(points, overshoot, width_cap, &missing_widths)?
    };
    Ok(Summary {
        width_cap,
        cap_bound,
        complete,
        missing_widths,
        incomplete_reason,
    })
}

/// `1600, 3200, 6400`, the way the artifact's prose lists widths.
fn list(widths: &[u32]) -> String {
    widths
        .iter()
        .map(|width| width.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// `1600 and 3200`: the same widths inside a sentence.
fn prose_list(widths: &[u32]) -> String {
    let parts: Vec<String> = widths.iter().map(|width| width.to_string()).collect();
    match parts.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// One sentence a reader can check: which width ended the climb, on what evidence,
/// and why the pre-registered points above it are absent.
fn stop_reason(
    points: &[Point],
    overshoot: Option<&Overshoot>,
    width_cap: u32,
    missing: &[u32],
) -> Result<String, String> {
    let stop_rule = "the sweep stops at the first width that fails a budget";

    // A probed width that is missing from the series is the stop rule's evidence.
    if let Some(probe) = overshoot.filter(|probe| probe.width > width_cap) {
        let mut reason = format!(
            "width {} was probed outside the curve and failed the default configuration's {} ms \
             p90 budget at {:.2} ms on {} ({} seeds)",
            probe.width,
            probe.budget_ms(),
            probe.ms_p90,
            probe.substrate,
            probe.samples
        );
        let note = probe.note.trim();
        if !note.is_empty() {
            reason.push_str(&format!("; {note}"));
        }
        reason.push_str("; it is recorded as the probe it is and not as a measured point");
        let rest: Vec<u32> = missing
            .iter()
            .copied()
            .filter(|width| *width != probe.width)
            .collect();
        if rest.is_empty() {
            reason.push_str("; every other pre-registered width was measured");
        } else {
            reason.push_str(&format!(
                "; {} {} not measured because {stop_rule}",
                prose_list(&rest),
                if rest.len() == 1 { "was" } else { "were" }
            ));
        }
        return Ok(reason);
    }

    // A measured width that failed: the point itself is the evidence, so the reason
    // is about the width above the cap rather than about a probe.
    let failing = points
        .iter()
        .filter(|point| point.width > width_cap)
        .find_map(|point| {
            point
                .cost
                .iter()
                .enumerate()
                .find(|(bed_index, bed)| bed.ms_p90 > CostRow::budget(*bed_index) as f64)
                .map(|(bed_index, bed)| (point, bed_index, bed))
        });
    if let Some((failed, bed_index, bed)) = failing {
        let rest: Vec<u32> = missing
            .iter()
            .copied()
            .filter(|width| *width != failed.width)
            .collect();
        let mut reason = format!(
            "width {} was measured and {} p90 {:.2} ms against its {} ms budget",
            failed.width,
            bed.bed,
            bed.ms_p90,
            CostRow::budget(bed_index)
        );
        if rest.is_empty() {
            reason.push_str("; it was the last pre-registered width");
        } else {
            reason.push_str(&format!(
                "; {} {} not measured because {stop_rule}",
                prose_list(&rest),
                if rest.len() == 1 { "was" } else { "were" }
            ));
        }
        return Ok(reason);
    }

    Err(format!(
        "widths {} are missing and the artifact records neither a failing width nor a probe: \
         there is no honest sentence for why the climb stopped",
        list(missing)
    ))
}

// ------------------------------------------------------------ the default width

/// One width's quality row on the ranking bed: the win tally and its paired
/// counts against the cap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QualityRow {
    pub width: u32,
    pub tally: Tally,
    /// Against the cap: `b` = cap won and this width lost, `c` = the reverse.
    pub paired: Paired,
}

/// Where the default came from. "The cap ties itself" is the degenerate case: it
/// is legal ("the default may equal the cap") and it is visible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultSource {
    /// A narrower measured width tied the cap.
    Tied,
    /// Only the cap tied (it always ties itself).
    SelfTie,
}

/// §9.5's pre-registered rule: the narrowest measured width, ascending, whose win
/// rate ties the cap's. `rows` must be ascending and end with the cap.
pub fn default_beam(rows: &[QualityRow]) -> (u32, DefaultSource) {
    let (cap, narrower) = rows
        .split_last()
        .expect("default_beam needs at least the cap's own row");
    for row in narrower {
        if tie(row.tally, cap.tally, row.paired) {
            return (row.width, DefaultSource::Tied);
        }
    }
    (cap.width, DefaultSource::SelfTie)
}

// ------------------------------------------------------------ the quality axis

/// Whether the bed that ranked the widths could rank anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QualityAxis {
    /// §9.1's pilot has not run yet: the artifact on disk is the in-flight copy a
    /// run rewrites after every cost point, so an interrupted run still leaves
    /// evidence. Never the state of a finished artifact — [`Artifact::validate`]
    /// refuses it.
    Pending,
    /// The bed has deaths to divide the widths, so the tie rule ranked them.
    Resolved,
    /// The bed is saturated (§9.1): the widths were named on cost, and the
    /// artifact says so.
    NoResolution,
}

impl QualityAxis {
    pub fn as_str(self) -> &'static str {
        match self {
            QualityAxis::Pending => "pending",
            QualityAxis::Resolved => "resolved",
            QualityAxis::NoResolution => "no-resolution",
        }
    }
}

/// What named `DEFAULT_BEAM`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultBasis {
    /// A measured quality tie with the cap.
    Quality,
    /// The ranking bed had no resolution, so the narrowest point was named on
    /// cost. Never described as a quality result.
    Cost,
}

impl DefaultBasis {
    pub fn as_str(self) -> &'static str {
        match self {
            DefaultBasis::Quality => "quality",
            DefaultBasis::Cost => "cost",
        }
    }
}

// ------------------------------------------------------------------ the clock

/// Seconds since the Unix epoch as `2026-11-02T14:02:11Z`. Hand-rolled because
/// `bench` has no dependencies and a timestamp is not worth one.
pub fn iso_utc(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let rem = seconds % 86_400;
    let (hour, minute, second) = (rem / 3_600, (rem % 3_600) / 60, rem % 60);

    // Civil-from-days (Howard Hinnant's algorithm), shifted to an epoch of 0000-03-01.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

// --------------------------------------------------------------- the artifact

/// One bed's cost row inside the artifact: the p90s, the trials that produced
/// them, and the wins the timed solves scored (the same solves answer both).
#[derive(Clone, Debug, PartialEq)]
pub struct BedCost {
    pub bed: String,
    pub matches: Tally,
    pub evaluations_p50: u32,
    pub ms_p50: f64,
    pub ms_p90: f64,
    pub ms_max: f64,
    /// Every trial's p90, so the spread is visible instead of laundered into one
    /// number.
    pub trials_ms_p90: Vec<f64>,
}

/// One measured doubling point.
#[derive(Clone, Debug, PartialEq)]
pub struct Point {
    pub width: u32,
    pub cost: Vec<BedCost>,
    /// Present when this width was measured on the ranking bed.
    pub quality: Option<QualityRow>,
    /// Whether the step from this width to the next measured quality width was
    /// flat (the stop rule's own verdict), when there is a next one.
    pub flat_step: Option<bool>,
}

/// §9.1's pilot: what the two shipped beds did at the cap.
#[derive(Clone, Debug, PartialEq)]
pub struct Saturation {
    pub checked_at_width: u32,
    /// `(bed name, tally at the cap)` in `Bed::SHIPPED` order.
    pub beds: Vec<(String, Tally)>,
    /// The bed that ranked the widths.
    pub quality_bed: String,
    /// The validation bed's own test split, or the shipped bed's.
    pub quality_split: (u32, u32),
    pub fallback_reason: String,
}

/// Everything the curve measured, in the shape the page and §9.6 read.
#[derive(Clone, Debug, PartialEq)]
pub struct Artifact {
    pub generated_at: String,
    pub engine_commit: String,
    pub wasm_bytes: u64,
    pub wasm_fingerprint: String,
    pub wasm_path: String,
    /// `(config dir, fingerprint, generator_version)` per shipped bed.
    pub datasets: Vec<(String, String, u32)>,
    pub evaluator_id: String,
    pub evaluator_name: String,
    pub evaluator_table: String,
    pub machine: String,
    pub runtime: String,
    pub substrate: String,
    pub widths: Vec<u32>,
    pub cost_trials: u32,
    pub cost_seeds_per_trial: u32,
    pub quality_games: u32,
    pub points: Vec<Point>,
    pub saturation: Saturation,
    pub quality_axis: QualityAxis,
    pub default_beam: u32,
    pub default_beam_source: DefaultSource,
    pub default_beam_basis: DefaultBasis,
    pub width_cap: u32,
    pub cap_bound: CapBound,
    /// The probe that failed a budget above the cap, when the stop rule was
    /// confirmed that way rather than by measuring the failing point. Never a
    /// point, never in the series: see [`Overshoot`].
    pub overshoot: Option<Overshoot>,
    pub quality_fingerprint: String,
    pub reason: String,
    /// Every pre-registered doubling point is a measured point. A cost sweep that
    /// stopped at its pre-registered stop rule leaves this `false`: the wider
    /// points were deliberately not measured, and the artifact says so instead of
    /// implying a full list. Derived by [`summarise`], never from the widths the
    /// run was asked for.
    pub complete: bool,
    /// Pre-registered widths the run did not measure, in ascending order. Empty
    /// only when [`Artifact::complete`].
    pub missing_widths: Vec<u32>,
    /// Why those widths are missing: the stop rule's own evidence, or (on an
    /// in-flight copy) the phases that had not run yet. Empty when complete.
    pub incomplete_reason: String,
    /// The width at which §9.5's quality stop rule (two consecutive flat steps)
    /// fired, if it did. The series is measured to the cap anyway, so every cost
    /// point carries its wins — this records the rule's verdict, not a limit on
    /// the evidence.
    pub quality_stop_at: Option<u32>,
}

impl Artifact {
    /// The invariants the page depends on. A curve that fails this is not written.
    pub fn validate(&self) -> Result<(), String> {
        if self.points.is_empty() {
            return Err("the curve records no points".to_string());
        }
        if self
            .points
            .windows(2)
            .any(|window| window[0].width >= window[1].width)
        {
            return Err("points must be ascending by width".to_string());
        }
        if !WIDTHS.contains(&self.width_cap) {
            return Err(format!(
                "width_cap {} is not one of the pre-registered doubling points",
                self.width_cap
            ));
        }
        if !WIDTHS.contains(&self.default_beam) {
            return Err(format!(
                "default_beam {} is not one of the pre-registered doubling points",
                self.default_beam
            ));
        }
        if self.default_beam < 1 || self.default_beam > self.width_cap {
            return Err(format!(
                "default_beam {} must be at least 1 and at most the cap {}",
                self.default_beam, self.width_cap
            ));
        }
        if !self
            .points
            .iter()
            .any(|point| point.width == self.width_cap)
        {
            return Err("the cap is not a measured point".to_string());
        }
        if !self
            .points
            .iter()
            .any(|point| point.width == self.default_beam)
        {
            return Err("the default is not a measured point".to_string());
        }
        for point in &self.points {
            if point.cost.len() != Bed::SHIPPED.len() {
                return Err(format!(
                    "width {} does not carry both shipped beds",
                    point.width
                ));
            }
            for (bed, shipped) in point.cost.iter().zip(Bed::SHIPPED.iter()) {
                if bed.bed != shipped.name() {
                    return Err(format!(
                        "width {}: cost rows are in shipped order, found {} where {} belongs",
                        point.width,
                        bed.bed,
                        shipped.name()
                    ));
                }
                if !bed.ms_p90.is_finite()
                    || !bed.ms_p50.is_finite()
                    || bed.ms_p90 < bed.ms_p50
                    || bed.ms_p90 <= 0.0
                {
                    return Err(format!(
                        "width {} {}: p90 {} must be positive and at least p50 {}",
                        point.width, bed.bed, bed.ms_p90, bed.ms_p50
                    ));
                }
            }
        }
        // Coverage is derived from the measured points against §9.5's list, not
        // taken on trust. `complete` and `missing_widths` are the fields the
        // committed defect got wrong (they were computed from the widths a run was
        // *asked* for), so a curve that claims `complete: true` over a six-point
        // prefix must be refused — even when its own fields agree with each other,
        // which is precisely what a wrong derivation produces.
        let unmeasured: Vec<u32> = WIDTHS
            .iter()
            .copied()
            .filter(|width| !self.points.iter().any(|point| point.width == *width))
            .collect();
        if self.missing_widths != unmeasured {
            return Err(format!(
                "missing_widths [{}] is not the pre-registered list minus the measured points \
                 ([{}]): coverage is derived from the doubling list, never from the widths a run \
                 asked for",
                list(&self.missing_widths),
                list(&unmeasured)
            ));
        }
        // What stopped the climb is a claim about evidence, so it is checked
        // against the evidence: a measured failing width above the cap, or a probe
        // there. Neither field may be taken on trust, because the page's tooltip
        // renders the difference (§2.2).
        match self.cap_bound {
            CapBound::Budget => {
                let measured_failure =
                    self.points.iter().any(|point| {
                        point.width > self.width_cap
                            && point.cost.iter().enumerate().any(|(bed_index, bed)| {
                                bed.ms_p90 > CostRow::budget(bed_index) as f64
                            })
                    });
                let probed = self
                    .overshoot
                    .as_ref()
                    .is_some_and(|probe| probe.width > self.width_cap);
                if !measured_failure && !probed {
                    return Err(
                        "cap_bound is \"budget\" but neither a measured point above the cap nor an \
                         overshoot probe failed a budget"
                            .to_string(),
                    );
                }
            }
            CapBound::Schedule => {
                if self.overshoot.is_some() {
                    return Err(
                        "cap_bound is \"schedule\" but the artifact records an overshoot probe that \
                         failed a budget: a probe that fails a budget is a budget"
                            .to_string(),
                    );
                }
                if !self.complete {
                    return Err(
                        "cap_bound is \"schedule\" says the pre-registered list stopped the climb, \
                         but this curve did not measure every pre-registered width"
                            .to_string(),
                    );
                }
            }
        }
        if let Some(probe) = &self.overshoot {
            probe.check(self.width_cap)?;
            if self.points.iter().any(|point| point.width == probe.width) {
                return Err(format!(
                    "width {} is recorded both as a measured point and as an overshoot probe: a \
                     probe is a width the curve did not measure",
                    probe.width
                ));
            }
            if self.complete {
                return Err(
                    "a complete curve measured every pre-registered width, so it cannot also carry \
                     an overshoot probe"
                        .to_string(),
                );
            }
            if !self.missing_widths.contains(&probe.width) {
                return Err(format!(
                    "the overshoot probe at width {} is not one of the widths this curve says it \
                     did not measure, so the probe would be a point the series is missing",
                    probe.width
                ));
            }
        }
        if self.saturation.checked_at_width != self.width_cap {
            return Err("§9.1's pilot must be checked at the cap".to_string());
        }
        if self.saturation.beds.len() != Bed::SHIPPED.len() {
            return Err("the pilot must record both shipped beds".to_string());
        }
        if self.saturation.quality_bed.is_empty() || self.saturation.fallback_reason.is_empty() {
            return Err("the artifact must say which bed ranked the widths and why".to_string());
        }
        if self.quality_axis == QualityAxis::Pending {
            return Err(
                "the quality axis is pending: §9.1's pilot and the quality series have not run"
                    .to_string(),
            );
        }
        if self.quality_axis == QualityAxis::NoResolution
            && self.default_beam_basis != DefaultBasis::Cost
        {
            return Err(
                "a curve with no quality resolution must name its default on cost".to_string(),
            );
        }
        if self.quality_axis == QualityAxis::Resolved
            && self.default_beam_basis != DefaultBasis::Quality
        {
            return Err("a resolved quality axis must name its default on quality".to_string());
        }
        if self.complete != self.missing_widths.is_empty() {
            return Err(if self.complete {
                "a complete curve measured every doubling point, so nothing can be missing"
                    .to_string()
            } else {
                "a partial curve must say which widths are missing".to_string()
            });
        }
        let explains = !self.incomplete_reason.trim().is_empty();
        if self.complete == explains {
            return Err(if self.complete {
                "a complete curve has nothing to explain".to_string()
            } else {
                "a partial curve must say why the widths are missing".to_string()
            });
        }
        // §9.5's tie is judged against the cap, so every width at or below it must
        // carry the wins that judgement was made from: a curve that names a default
        // out of a series it did not record is not evidence.
        for point in self.points.iter().filter(|p| p.width <= self.width_cap) {
            if point.quality.is_none() {
                return Err(format!(
                    "width {} is at or below the cap {} but carries no quality row",
                    point.width, self.width_cap
                ));
            }
        }
        if self.reason.trim().is_empty() {
            return Err("a measurement must carry its one-line reason".to_string());
        }
        Ok(())
    }

    /// Recompute the derived fields from the evidence this artifact already holds —
    /// its measured points and its probe — and nothing else. This is the single
    /// derivation a live run and a re-derivation of a committed file both go
    /// through, so a curve cannot be summarised two ways.
    pub fn refresh_summary(&mut self) -> Result<(), String> {
        let summary = summarise(&self.points, self.overshoot.as_ref())?;
        self.width_cap = summary.width_cap;
        self.cap_bound = summary.cap_bound;
        self.complete = summary.complete;
        self.missing_widths = summary.missing_widths;
        self.incomplete_reason = summary.incomplete_reason;
        Ok(())
    }

    /// Two-space JSON, `\n` at EOF: the same convention as the manifests.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str("  \"format\": 1,\n");
        out.push_str("  \"kind\": \"width-curve\",\n");
        line(&mut out, "generated_at", &quote(&self.generated_at), 1);
        out.push_str("  \"engine\": {\n");
        line(&mut out, "commit", &quote(&self.engine_commit), 2);
        line(
            &mut out,
            "profile",
            &quote("release: lto=fat, codegen-units=1, panic=abort, strip=true"),
            2,
        );
        out.push_str("    \"wasm\": {\n");
        line(&mut out, "path", &quote(&self.wasm_path), 3);
        line(&mut out, "bytes", &self.wasm_bytes.to_string(), 3);
        line(&mut out, "fingerprint", &quote(&self.wasm_fingerprint), 3);
        seal(&mut out);
        out.push_str("    }\n");
        out.push_str("  },\n");

        out.push_str("  \"dataset\": {\n");
        for (index, (name, fingerprint, version)) in self.datasets.iter().enumerate() {
            out.push_str(&format!("    {}: {{", quote(name)));
            out.push_str(&format!(" \"fingerprint\": {}", quote(fingerprint)));
            out.push_str(&format!(", \"generator_version\": {version} }}"));
            out.push_str(if index + 1 == self.datasets.len() {
                "\n"
            } else {
                ",\n"
            });
        }
        out.push_str("  },\n");

        out.push_str("  \"evaluator\": {\n");
        line(&mut out, "id", &quote(&self.evaluator_id), 2);
        line(&mut out, "name", &quote(&self.evaluator_name), 2);
        line(&mut out, "table", &quote(&self.evaluator_table), 2);
        line(&mut out, "toggles", &quote("shipped"), 2);
        seal(&mut out);
        out.push_str("  },\n");

        out.push_str("  \"machine\": {\n");
        line(&mut out, "label", &quote(&self.machine), 2);
        line(&mut out, "runtime", &quote(&self.runtime), 2);
        line(&mut out, "workers", "1", 2);
        seal(&mut out);
        out.push_str("  },\n");

        // ---- protocol
        out.push_str("  \"protocol\": {\n");
        out.push_str(&format!(
            "    \"widths\": [{}],\n",
            self.widths
                .iter()
                .map(|width| width.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        out.push_str(&format!(
            "    \"cost_split\": {{ \"first\": {}, \"last\": {} }},\n",
            TEST_SPLIT.0, TEST_SPLIT.1
        ));
        out.push_str(&format!(
            "    \"cost_trials\": {},\n    \"cost_seeds_per_trial\": {},\n    \"cost_games_per_point\": {},\n",
            self.cost_trials,
            self.cost_seeds_per_trial,
            self.cost_trials * self.cost_seeds_per_trial
        ));
        line(
            &mut out,
            "quality_bed",
            &quote(&self.saturation.quality_bed),
            2,
        );
        out.push_str(&format!(
            "    \"quality_split\": {{ \"first\": {}, \"last\": {} }},\n",
            self.saturation.quality_split.0, self.saturation.quality_split.1
        ));
        line(
            &mut out,
            "quality_games_per_point",
            &self.quality_games.to_string(),
            2,
        );
        match self.quality_stop_at {
            Some(width) => line(
                &mut out,
                "quality_stop_rule_fired_at",
                &width.to_string(),
                2,
            ),
            None => line(&mut out, "quality_stop_rule_fired_at", "null", 2),
        }
        line(&mut out, "p90_method", &quote("nearest-rank"), 2);
        line(&mut out, "substrate", &quote(&self.substrate), 2);
        line(
            &mut out,
            "quality_substrate",
            &quote("native (win counts are engine-deterministic; the wasm/native parity gate holds them)"),
            2,
        );
        line(
            &mut out,
            "quality_stop_rule",
            &quote("two consecutive flat steps, a flat step being the §9.5 tie predicate"),
            2,
        );
        line(
            &mut out,
            "cost_stop_rule",
            &quote("the first width that fails either budget"),
            2,
        );
        line(
            &mut out,
            "ranking_rule",
            &quote("§9.5: narrowest doubling point whose win rate ties the cap's; ties resolve narrower"),
            2,
        );
        seal(&mut out);
        out.push_str("  },\n");

        // ---- statistic
        out.push_str("  \"statistic\": {\n");
        line(
            &mut out,
            "name",
            &quote("exact McNemar (two-sided binomial sign test on the discordant pairs)"),
            2,
        );
        line(&mut out, "alpha", "0.01", 2);
        line(&mut out, "max_shortfall_pp", "1.0", 2);
        line(
            &mut out,
            "max_shortfall_tenths_pp",
            &MAX_SHORTFALL_TENTHS_PP.to_string(),
            2,
        );
        line(
            &mut out,
            "saturation_percent",
            &SATURATION_PERCENT.to_string(),
            2,
        );
        out.push_str("    \"budgets_ms\": {\n");
        line(&mut out, "default_bed", &quote(&Bed::SHIPPED[0].name()), 3);
        line(&mut out, "default_ms", &DEFAULT_BUDGET_MS.to_string(), 3);
        line(&mut out, "harder_bed", &quote(&Bed::SHIPPED[1].name()), 3);
        line(&mut out, "harder_ms", &HARDER_BUDGET_MS.to_string(), 3);
        seal(&mut out);
        out.push_str("    }\n");
        out.push_str("  },\n");

        // ---- saturation
        out.push_str("  \"saturation\": {\n");
        line(
            &mut out,
            "checked_at_width",
            &self.saturation.checked_at_width.to_string(),
            2,
        );
        line(
            &mut out,
            "saturated",
            if self
                .saturation
                .beds
                .iter()
                .any(|(_, tally)| tally.saturated())
            {
                "true"
            } else {
                "false"
            },
            2,
        );
        if self.saturation.beds.is_empty() {
            out.push_str("    \"beds\": {},\n");
        } else {
            out.push_str("    \"beds\": {\n");
            for (index, (name, tally)) in self.saturation.beds.iter().enumerate() {
                out.push_str(&format!(
                    "      {}: {{ \"wins\": {}, \"games\": {} }}",
                    quote(name),
                    tally.wins,
                    tally.games
                ));
                out.push_str(if index + 1 == self.saturation.beds.len() {
                    "\n"
                } else {
                    ",\n"
                });
            }
            out.push_str("    },\n");
        }
        line(
            &mut out,
            "quality_bed",
            &quote(&self.saturation.quality_bed),
            2,
        );
        line(
            &mut out,
            "quality_split",
            &format!(
                "{{ \"first\": {}, \"last\": {} }}",
                self.saturation.quality_split.0, self.saturation.quality_split.1
            ),
            2,
        );
        line(
            &mut out,
            "fallback_reason",
            &quote(&self.saturation.fallback_reason),
            2,
        );
        seal(&mut out);
        out.push_str("  },\n");

        // ---- points
        out.push_str("  \"points\": [\n");
        for (index, point) in self.points.iter().enumerate() {
            out.push_str("    {\n");
            line(&mut out, "width", &point.width.to_string(), 3);
            out.push_str("      \"cost\": [\n");
            for (bed_index, bed) in point.cost.iter().enumerate() {
                out.push_str("        {\n");
                line(&mut out, "bed", &quote(&bed.bed), 5);
                line(&mut out, "wins", &bed.matches.wins.to_string(), 5);
                line(&mut out, "games", &bed.matches.games.to_string(), 5);
                line(
                    &mut out,
                    "evaluations_p50",
                    &bed.evaluations_p50.to_string(),
                    5,
                );
                line(&mut out, "ms_p50", &format!("{:.3}", bed.ms_p50), 5);
                line(&mut out, "ms_p90", &format!("{:.3}", bed.ms_p90), 5);
                line(&mut out, "ms_max", &format!("{:.3}", bed.ms_max), 5);
                out.push_str(&format!(
                    "          \"trials_ms_p90\": [{}]\n",
                    bed.trials_ms_p90
                        .iter()
                        .map(|ms| format!("{ms:.3}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
                out.push_str(if bed_index + 1 == point.cost.len() {
                    "        }\n"
                } else {
                    "        },\n"
                });
            }
            out.push_str("      ]");
            match point.quality {
                Some(quality) => {
                    out.push_str(",\n      \"quality\": {\n");
                    line(&mut out, "wins", &quality.tally.wins.to_string(), 4);
                    line(&mut out, "games", &quality.tally.games.to_string(), 4);
                    line(
                        &mut out,
                        "paired_vs_cap",
                        &format!(
                            "{{ \"b\": {}, \"c\": {}, \"n\": {} }}",
                            quality.paired.b,
                            quality.paired.c,
                            quality.paired.n()
                        ),
                        4,
                    );
                    let cap_tally = self
                        .points
                        .iter()
                        .filter_map(|point| point.quality)
                        .next_back()
                        .map(|row| row.tally)
                        .unwrap_or(quality.tally);
                    line(
                        &mut out,
                        "shortfall_wins_vs_cap",
                        &shortfall_wins(quality.tally, cap_tally).to_string(),
                        4,
                    );
                    line(
                        &mut out,
                        "saturated",
                        if quality.tally.saturated() {
                            "true"
                        } else {
                            "false"
                        },
                        4,
                    );
                    match point.flat_step {
                        Some(flat) => {
                            out.push_str(&format!(
                                "          \"flat_step\": {}\n",
                                if flat { "true" } else { "false" }
                            ));
                        }
                        None => out.push_str("          \"flat_step\": null\n"),
                    }
                    out.push_str("      }\n");
                }
                None => out.push('\n'),
            }
            out.push_str(if index + 1 == self.points.len() {
                "    }\n"
            } else {
                "    },\n"
            });
        }
        out.push_str("  ],\n");

        // ---- the two numbers
        line(&mut out, "default_beam", &self.default_beam.to_string(), 1);
        line(&mut out, "width_cap", &self.width_cap.to_string(), 1);
        let bound = match self.cap_bound {
            CapBound::Budget => "budget",
            CapBound::Schedule => "schedule",
        };
        line(&mut out, "cap_bound", &quote(bound), 1);
        // The probe, if any, goes beside the bound it justifies: it is evidence
        // about the stop rule, so a reader checking the word above finds the number
        // underneath it. `probe: true` is the field that keeps it out of `points`.
        match &self.overshoot {
            None => line(&mut out, "overshoot", "null", 1),
            Some(probe) => {
                out.push_str("  \"overshoot\": {\n");
                line(&mut out, "width", &probe.width.to_string(), 2);
                line(&mut out, "bed", &quote(&probe.bed()), 2);
                line(&mut out, "substrate", &quote(&probe.substrate), 2);
                line(&mut out, "samples", &probe.samples.to_string(), 2);
                // Two decimals, one fewer than the cost series: this is a probe,
                // not a point, and it should not look like one.
                line(&mut out, "ms_p90", &format!("{:.2}", probe.ms_p90), 2);
                line(&mut out, "budget_ms", &probe.budget_ms().to_string(), 2);
                line(&mut out, "probe", "true", 2);
                line(&mut out, "note", &quote(probe.note.trim()), 2);
                seal(&mut out);
                out.push_str("  },\n");
            }
        }
        let source = match self.default_beam_source {
            DefaultSource::Tied => "tie",
            DefaultSource::SelfTie => "self",
        };
        line(&mut out, "default_beam_source", &quote(source), 1);
        line(
            &mut out,
            "quality_axis",
            &quote(self.quality_axis.as_str()),
            1,
        );
        line(
            &mut out,
            "default_beam_basis",
            &quote(self.default_beam_basis.as_str()),
            1,
        );
        line(
            &mut out,
            "quality_fingerprint",
            &quote(&self.quality_fingerprint),
            1,
        );
        line(
            &mut out,
            "complete",
            if self.complete { "true" } else { "false" },
            1,
        );
        out.push_str(&format!(
            "  \"missing_widths\": [{}],\n",
            self.missing_widths
                .iter()
                .map(|width| width.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        line(
            &mut out,
            "incomplete_reason",
            &quote(&self.incomplete_reason),
            1,
        );
        out.push_str("  \"provenance\": {\n");
        line(&mut out, "reason", &quote(&self.reason), 2);
        out.push_str("    \"supersedes\": null\n");
        out.push_str("  }\n");
        out.push_str("}\n");
        out
    }

    /// Read an artifact back out of its own rendering.
    ///
    /// This exists for `--summarise`: the measured series is exactly the part a
    /// later command must never touch, so the reader is written against
    /// [`Artifact::render`] and the two are held together by a round-trip test on
    /// the committed file — parse then render has to reproduce it byte for byte.
    /// Anything the writer emits and the reader drops would show up there as a diff
    /// rather than as a measured number quietly changing.
    pub fn from_text(text: &str) -> Result<Artifact, String> {
        let root = json::parse(text).map_err(|error| format!("not JSON: {error}"))?;
        if json_count(&root, "format", "format")? != 1 {
            return Err("format: this reader knows format 1 only".to_string());
        }
        let kind = json_text(&root, "kind", "kind")?;
        if kind != "width-curve" {
            return Err(format!("kind: expected \"width-curve\", found {kind:?}"));
        }

        let engine = json_field(&root, "engine", "engine")?;
        let wasm = json_field(engine, "wasm", "engine.wasm")?;

        let datasets = json_field(&root, "dataset", "dataset")?
            .as_object()
            .ok_or_else(|| "dataset: expected an object".to_string())?
            .iter()
            .map(|(name, entry)| {
                let path = format!("dataset.{name}");
                Ok((
                    name.clone(),
                    json_text(entry, "fingerprint", &path)?,
                    json_count(entry, "generator_version", &path)?,
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;

        let evaluator = json_field(&root, "evaluator", "evaluator")?;
        let machine = json_field(&root, "machine", "machine")?;
        let protocol = json_field(&root, "protocol", "protocol")?;
        let saturation = json_field(&root, "saturation", "saturation")?;

        let mut widths = Vec::new();
        for (index, width) in json_field(protocol, "widths", "protocol")?
            .as_array()
            .ok_or_else(|| "protocol.widths: expected an array".to_string())?
            .iter()
            .enumerate()
        {
            widths.push(json_u32_of(width, &format!("protocol.widths[{index}]"))?);
        }
        let quality_stop_at = match json_field(protocol, "quality_stop_rule_fired_at", "protocol")?
        {
            Value::Null => None,
            value => Some(json_u32_of(value, "protocol.quality_stop_rule_fired_at")?),
        };

        let mut beds = Vec::new();
        for (name, tally) in json_field(saturation, "beds", "saturation")?
            .as_object()
            .ok_or_else(|| "saturation.beds: expected an object".to_string())?
        {
            let path = format!("saturation.beds.{name}");
            beds.push((
                name.clone(),
                Tally {
                    wins: json_count(tally, "wins", &path)?,
                    games: json_count(tally, "games", &path)?,
                },
            ));
        }
        let quality_split = json_split(saturation, "quality_split", "saturation")?;

        let mut points = Vec::new();
        for (index, point) in json_field(&root, "points", "points")?
            .as_array()
            .ok_or_else(|| "points: expected an array".to_string())?
            .iter()
            .enumerate()
        {
            let path = format!("points[{index}]");
            let width = json_count(point, "width", &path)?;
            let mut cost = Vec::new();
            for (bed_index, bed) in json_field(point, "cost", &path)?
                .as_array()
                .ok_or_else(|| format!("{path}.cost: expected an array"))?
                .iter()
                .enumerate()
            {
                let bed_path = format!("{path}.cost[{bed_index}]");
                let mut trials = Vec::new();
                for (trial_index, ms) in json_field(bed, "trials_ms_p90", &bed_path)?
                    .as_array()
                    .ok_or_else(|| format!("{bed_path}.trials_ms_p90: expected an array"))?
                    .iter()
                    .enumerate()
                {
                    trials.push(json_number_of(
                        ms,
                        &format!("{bed_path}.trials_ms_p90[{trial_index}]"),
                    )?);
                }
                cost.push(BedCost {
                    bed: json_text(bed, "bed", &bed_path)?,
                    matches: Tally {
                        wins: json_count(bed, "wins", &bed_path)?,
                        games: json_count(bed, "games", &bed_path)?,
                    },
                    evaluations_p50: json_count(bed, "evaluations_p50", &bed_path)?,
                    ms_p50: json_number(bed, "ms_p50", &bed_path)?,
                    ms_p90: json_number(bed, "ms_p90", &bed_path)?,
                    ms_max: json_number(bed, "ms_max", &bed_path)?,
                    trials_ms_p90: trials,
                });
            }
            let quality_node = point.get("quality");
            let quality = match quality_node {
                None | Some(Value::Null) => None,
                Some(node) => {
                    let quality_path = format!("{path}.quality");
                    let paired = json_field(node, "paired_vs_cap", &quality_path)?;
                    Some(QualityRow {
                        width,
                        tally: Tally {
                            wins: json_count(node, "wins", &quality_path)?,
                            games: json_count(node, "games", &quality_path)?,
                        },
                        paired: Paired {
                            b: json_count(paired, "b", &format!("{quality_path}.paired_vs_cap"))?,
                            c: json_count(paired, "c", &format!("{quality_path}.paired_vs_cap"))?,
                        },
                    })
                }
            };
            // `flat_step` is the quality block's own verdict on the step above it, so
            // it is read from there — where [`Artifact::render`] writes it.
            let flat_step = match quality_node.and_then(|node| node.get("flat_step")) {
                None | Some(Value::Null) => None,
                Some(value) => Some(value.as_bool().ok_or_else(|| {
                    format!("{path}.quality.flat_step: expected a boolean or null")
                })?),
            };
            points.push(Point {
                width,
                cost,
                quality,
                flat_step,
            });
        }

        let overshoot = match root.get("overshoot") {
            None | Some(Value::Null) => None,
            Some(probe) => {
                let path = "overshoot";
                if let Some(bed) = probe.get("bed") {
                    let bed = bed
                        .as_str()
                        .ok_or_else(|| format!("{path}.bed: expected a string"))?;
                    if bed != Bed::SHIPPED[0].name() {
                        return Err(format!(
                            "{path}.bed: expected {:?}, found {bed:?} — §9.5's cap criterion is \
                             the default configuration's budget",
                            Bed::SHIPPED[0].name()
                        ));
                    }
                }
                if probe.get("budget_ms").is_some()
                    && json_count(probe, "budget_ms", path)? != BUDGETS_MS[0] as u32
                {
                    return Err(format!("{path}.budget_ms: expected {}", BUDGETS_MS[0]));
                }
                if probe.get("probe").is_some() && !json_bool(probe, "probe", path)? {
                    return Err(format!(
                        "{path}.probe: false — an overshoot that is not a probe is a point, and a \
                         point belongs in the series"
                    ));
                }
                Some(Overshoot {
                    width: json_count(probe, "width", path)?,
                    substrate: json_text(probe, "substrate", path)?,
                    samples: json_count(probe, "samples", path)?,
                    ms_p90: json_number(probe, "ms_p90", path)?,
                    note: json_text(probe, "note", path)?,
                })
            }
        };

        let cap_bound = match json_text(&root, "cap_bound", "cap_bound")?.as_str() {
            "budget" => CapBound::Budget,
            "schedule" => CapBound::Schedule,
            other => {
                return Err(format!(
                    "cap_bound: expected \"budget\" or \"schedule\", found {other:?}"
                ))
            }
        };
        let default_beam_source =
            match json_text(&root, "default_beam_source", "default_beam_source")?.as_str() {
                "tie" => DefaultSource::Tied,
                "self" => DefaultSource::SelfTie,
                other => {
                    return Err(format!(
                        "default_beam_source: expected \"tie\" or \"self\", found {other:?}"
                    ))
                }
            };
        let quality_axis = match json_text(&root, "quality_axis", "quality_axis")?.as_str() {
            "pending" => QualityAxis::Pending,
            "resolved" => QualityAxis::Resolved,
            "no-resolution" => QualityAxis::NoResolution,
            other => {
                return Err(format!(
                    "quality_axis: expected \"pending\", \"resolved\" or \"no-resolution\", found \
                     {other:?}"
                ))
            }
        };
        let default_beam_basis =
            match json_text(&root, "default_beam_basis", "default_beam_basis")?.as_str() {
                "quality" => DefaultBasis::Quality,
                "cost" => DefaultBasis::Cost,
                other => {
                    return Err(format!(
                        "default_beam_basis: expected \"quality\" or \"cost\", found {other:?}"
                    ))
                }
            };
        let mut missing_widths = Vec::new();
        for (index, width) in json_field(&root, "missing_widths", "missing_widths")?
            .as_array()
            .ok_or_else(|| "missing_widths: expected an array".to_string())?
            .iter()
            .enumerate()
        {
            missing_widths.push(json_u32_of(width, &format!("missing_widths[{index}]"))?);
        }

        Ok(Artifact {
            generated_at: json_text(&root, "generated_at", "generated_at")?,
            engine_commit: json_text(engine, "commit", "engine")?,
            wasm_bytes: json_count(wasm, "bytes", "engine.wasm")? as u64,
            wasm_fingerprint: json_text(wasm, "fingerprint", "engine.wasm")?,
            wasm_path: json_text(wasm, "path", "engine.wasm")?,
            datasets,
            evaluator_id: json_text(evaluator, "id", "evaluator")?,
            evaluator_name: json_text(evaluator, "name", "evaluator")?,
            evaluator_table: json_text(evaluator, "table", "evaluator")?,
            machine: json_text(machine, "label", "machine")?,
            runtime: json_text(machine, "runtime", "machine")?,
            substrate: json_text(protocol, "substrate", "protocol")?,
            widths,
            cost_trials: json_count(protocol, "cost_trials", "protocol")?,
            cost_seeds_per_trial: json_count(protocol, "cost_seeds_per_trial", "protocol")?,
            quality_games: json_count(protocol, "quality_games_per_point", "protocol")?,
            points,
            saturation: Saturation {
                checked_at_width: json_count(saturation, "checked_at_width", "saturation")?,
                beds,
                quality_bed: json_text(saturation, "quality_bed", "saturation")?,
                quality_split,
                fallback_reason: json_text(saturation, "fallback_reason", "saturation")?,
            },
            quality_axis,
            default_beam: json_count(&root, "default_beam", "default_beam")?,
            default_beam_source,
            default_beam_basis,
            width_cap: json_count(&root, "width_cap", "width_cap")?,
            cap_bound,
            overshoot,
            quality_fingerprint: json_text(&root, "quality_fingerprint", "quality_fingerprint")?,
            reason: json_text(
                json_field(&root, "provenance", "provenance")?,
                "reason",
                "provenance",
            )?,
            complete: json_bool(&root, "complete", "complete")?,
            missing_widths,
            incomplete_reason: json_text(&root, "incomplete_reason", "incomplete_reason")?,
            quality_stop_at,
        })
    }
}

// ------------------------------------------------- reading the artifact's fields

fn json_field<'a>(value: &'a Value, key: &str, path: &str) -> Result<&'a Value, String> {
    value
        .get(key)
        .ok_or_else(|| format!("{path}: missing {key:?}"))
}

fn json_text(value: &Value, key: &str, path: &str) -> Result<String, String> {
    match json_field(value, key, path)? {
        Value::String(text) => Ok(text.clone()),
        other => Err(format!(
            "{path}.{key}: expected a string, found {}",
            other.kind()
        )),
    }
}

fn json_count(value: &Value, key: &str, path: &str) -> Result<u32, String> {
    let found = json_field(value, key, path)?;
    json_u32_of(found, &format!("{path}.{key}"))
}

fn json_u32_of(value: &Value, path: &str) -> Result<u32, String> {
    value
        .as_u32()
        .ok_or_else(|| format!("{path}: expected a whole number, found {}", value.kind()))
}

fn json_number(value: &Value, key: &str, path: &str) -> Result<f64, String> {
    json_number_of(json_field(value, key, path)?, &format!("{path}.{key}"))
}

fn json_number_of(value: &Value, path: &str) -> Result<f64, String> {
    value
        .as_f64()
        .ok_or_else(|| format!("{path}: expected a number, found {}", value.kind()))
}

fn json_bool(value: &Value, key: &str, path: &str) -> Result<bool, String> {
    let found = json_field(value, key, path)?;
    found
        .as_bool()
        .ok_or_else(|| format!("{path}.{key}: expected a boolean, found {}", found.kind()))
}

fn json_split(value: &Value, key: &str, path: &str) -> Result<(u32, u32), String> {
    let split = json_field(value, key, path)?;
    let at = format!("{path}.{key}");
    Ok((
        json_count(split, "first", &at)?,
        json_count(split, "last", &at)?,
    ))
}

fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if (other as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", other as u32)),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn line(out: &mut String, key: &str, value: &str, depth: usize) {
    out.push_str(&"  ".repeat(depth));
    out.push_str(&quote(key));
    out.push_str(": ");
    out.push_str(value);
    out.push_str(",\n");
}

/// Drop the trailing comma `line` left behind, so a block can close. A hand-rolled
/// writer is one stray comma away from a file the page cannot parse, so every
/// block's closing brace goes through here.
fn seal(out: &mut String) {
    if out.ends_with(",\n") {
        out.truncate(out.len() - 2);
        out.push('\n');
    }
}

/// Read one integer field out of the committed artifact without a JSON parser:
/// the page parses it properly (`web/test/width-curve.test.mjs`), and this is the
/// bench's own read-back for the two numbers it promises.
pub fn json_i64(text: &str, key: &str) -> Option<i64> {
    let needle = format!("\"{key}\":");
    let start = text.find(&needle)? + needle.len();
    let rest = text[start..].trim_start();
    let end = rest
        .find(|character: char| !(character.is_ascii_digit() || character == '-'))
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// §9.6's rule applied to the curve: a quality row that moves under an identical
/// engine hash and Dataset fingerprint is a behaviour change, not a
/// re-measurement. Milliseconds are excluded by construction — they differ on
/// every run.
pub fn quality_fingerprint(points: &[Point]) -> String {
    let mut hash = Fnv::new();
    for point in points {
        hash.u32_le(point.width);
        for bed in &point.cost {
            for byte in bed.bed.as_bytes() {
                hash.byte(*byte);
            }
            hash.u32_le(bed.matches.wins);
            hash.u32_le(bed.matches.games);
        }
        match point.quality {
            Some(quality) => {
                hash.byte(1);
                hash.u32_le(quality.width);
                hash.u32_le(quality.tally.wins);
                hash.u32_le(quality.tally.games);
                hash.u32_le(quality.paired.b);
                hash.u32_le(quality.paired.c);
            }
            None => hash.byte(0),
        }
    }
    format!("fnv1a64:{}", fingerprint::hex(hash.finish()))
}

// ------------------------------------------------------- the re-derivation

/// What a `--summarise` run changed: the same artifact, with the summary fields it
/// should have had, and the fields it had before. Both are reported, because a
/// command that rewrites a committed measurement must be auditable on its own
/// output.
#[derive(Clone, Debug, PartialEq)]
pub struct Resummary {
    pub before: Summary,
    pub after: Summary,
    pub artifact: Artifact,
}

/// Recompute an artifact's derived summary fields from the evidence it already
/// records — no solving, no clock, no Dataset — and refuse to write anything that
/// would move a measured number.
///
/// This is the honest repair for a curve whose derived fields came out wrong: the
/// cost and quality series are the expensive, measured part, and re-deriving four
/// fields must not mean paying for them again (and must not tempt anyone to hand-edit
/// the file, which is the failure mode this command exists to prevent).
///
/// `overshoot` supplies the probe, because a probe may be evidence a previous run
/// recorded only in prose: pass it here to put it in the file. When it is `None` the
/// artifact's own probe is used, so re-summarising with the same evidence is a
/// no-op that changes nothing (byte for byte).
pub fn summarise_file(
    path: &Path,
    out: &Path,
    overshoot: Option<Overshoot>,
) -> Result<Resummary, String> {
    let text =
        fs::read_to_string(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let mut artifact =
        Artifact::from_text(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    let before = Summary {
        width_cap: artifact.width_cap,
        cap_bound: artifact.cap_bound,
        complete: artifact.complete,
        missing_widths: artifact.missing_widths.clone(),
        incomplete_reason: artifact.incomplete_reason.clone(),
    };
    if let Some(overshoot) = overshoot {
        artifact.overshoot = Some(overshoot);
    }
    artifact.refresh_summary()?;
    artifact.validate()?;

    let rendered = with_line_endings(&artifact.render(), &text);
    // The one guarantee that makes this command safe on a committed measurement:
    // everything outside the derived summary block has to come back byte for byte.
    match (evidence_regions(&rendered), evidence_regions(&text)) {
        (Some((rendered_evidence, rendered_tail)), Some((evidence, tail)))
            if rendered_evidence == evidence && rendered_tail == tail => {}
        _ => {
            return Err(format!(
                "refusing to summarise {}: re-rendering it would move a measured number ({}), and \
                 this command may only rewrite the derived summary fields",
                path.display(),
                first_difference(&rendered, &text)
            ))
        }
    }

    let after = Summary {
        width_cap: artifact.width_cap,
        cap_bound: artifact.cap_bound,
        complete: artifact.complete,
        missing_widths: artifact.missing_widths.clone(),
        incomplete_reason: artifact.incomplete_reason.clone(),
    };
    write_text(&rendered, out)?;
    Ok(Resummary {
        before,
        after,
        artifact,
    })
}

/// Match the line endings the artifact was checked out with.
///
/// The writer emits LF (the manifests' convention), but `core.autocrlf` can hand a
/// worktree a CRLF copy of the same committed file. Rewriting every line ending
/// would turn a one-field repair into a whole-file change, so the re-derivation
/// keeps the convention of the file it read and the guard compares real bytes.
fn with_line_endings(rendered: &str, source: &str) -> String {
    if source.contains("\r\n") {
        rendered.replace('\n', "\r\n")
    } else {
        rendered.to_string()
    }
}

/// Where a rendering's derived summary block starts and ends. Outside it is
/// evidence: identity, protocol, statistic, §9.1's pilot, the measured points and
/// the provenance. `--summarise` may only rewrite what is inside.
fn evidence_regions(text: &str) -> Option<(&str, &str)> {
    let summary = text.find(SUMMARY_START)?;
    let tail = text.find(PROVENANCE_START)?;
    if tail < summary {
        return None;
    }
    Some((&text[..summary], &text[tail..]))
}

/// The first line two renderings disagree on, for the guard's refusal: "refusing"
/// without saying which number it was protecting is not a diagnosis.
fn first_difference(rendered: &str, original: &str) -> String {
    for (index, (generated, found)) in rendered.lines().zip(original.lines()).enumerate() {
        if generated != found {
            return format!(
                "line {}: wrote {generated:?}, the file has {found:?}",
                index + 1
            );
        }
    }
    let generated = rendered.lines().count();
    let found = original.lines().count();
    if generated != found {
        return format!("{generated} lines written against {found} in the file");
    }
    "the same lines, differing in their line endings".to_string()
}

/// The first line of [`Artifact::render`]'s derived block.
const SUMMARY_START: &str = "  \"default_beam\": ";
/// The last block of a rendering, which the derived fields sit in front of.
const PROVENANCE_START: &str = "  \"provenance\": {";

// ------------------------------------------------------------------ the run

/// Where the milliseconds come from. §9.3's budgets are browser budgets, so the
/// cap is named on wasm; `native` exists for tests and for a sanity check, and
/// the artifact records which substrate produced its numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MsSource {
    Native,
    Wasm,
}

/// What to measure and where to write it.
pub struct Options {
    pub data: PathBuf,
    pub out: PathBuf,
    pub widths: Vec<u32>,
    /// Repeated trials per cost point. At least three on a shared box.
    pub trials: u32,
    /// Seeds per trial. The same seeds in every trial: the same work, repeated,
    /// so the spread is the machine's and not the sample's.
    pub trial_seeds: u32,
    /// Seeds in the ranking sweep (paired per seed).
    pub quality_games: u32,
    pub ms_source: MsSource,
    pub engine_commit: String,
    pub machine: String,
    pub runtime: String,
    pub reason: String,
    pub quiet: bool,
    pub now_secs: u64,
    /// The probe that failed a budget above the cap, if the operator has one. It is
    /// recorded beside the bound it justifies and never as a point; a run whose
    /// stop has no such evidence and no measured failing width is refused, because
    /// the artifact could not say what stopped the climb.
    pub overshoot: Option<Overshoot>,
}

/// One solve the curve asked for, and what came back.
#[derive(Clone, Copy, Debug)]
struct Sample {
    won: bool,
    evaluations: u32,
    ms: f64,
}

/// The games of one bed, in seed order: the committed Dataset when there is one,
/// and `engine::generate_game` when there is not (the validation bed is
/// deliberately not committed data — §9.1 says it never ships).
struct Games {
    bed: Bed,
    seeds: Vec<u32>,
    boards: Vec<engine::Board>,
    sequences: Vec<Vec<engine::Piece>>,
}

impl Games {
    fn load(bed: Bed, data: &Path, split: (u32, u32)) -> Result<Games, String> {
        match bed.config() {
            Some(config) => {
                let path = dataset::games_path(data, &config);
                let text = fs::read_to_string(&path)
                    .map_err(|error| format!("read {}: {error}", path.display()))?;
                let parsed = dataset::parse(&config, &text)
                    .map_err(|error| format!("parse {}: {error}", path.display()))?;
                let mut seeds = Vec::new();
                let mut boards = Vec::new();
                let mut sequences = Vec::new();
                for game in &parsed.games {
                    if game.seed < split.0 || game.seed > split.1 {
                        continue;
                    }
                    let sequence = parse_letters(&game.sequence).ok_or_else(|| {
                        format!(
                            "{}: seed {} has a sequence byte that is not a piece",
                            path.display(),
                            game.seed
                        )
                    })?;
                    seeds.push(game.seed);
                    boards.push(game.board());
                    sequences.push(sequence);
                }
                Ok(Games {
                    bed,
                    seeds,
                    boards,
                    sequences,
                })
            }
            None => {
                let mut seeds = Vec::new();
                let mut boards = Vec::new();
                let mut sequences = Vec::new();
                for seed in split.0..=split.1 {
                    let game = engine::generate_game(seed, bed.pieces, bed.cap)
                        .map_err(|error| format!("generate seed {seed}: {error:?}"))?;
                    seeds.push(seed);
                    boards.push(game.board);
                    sequences.push(game.sequence.as_slice().to_vec());
                }
                Ok(Games {
                    bed,
                    seeds,
                    boards,
                    sequences,
                })
            }
        }
    }

    fn len(&self) -> usize {
        self.seeds.len()
    }

    fn solve(&self, index: usize, width: u32) -> Sample {
        let started = Instant::now();
        let result = engine::solve(
            &self.boards[index],
            &self.sequences[index],
            &engine::SolveConfig::new(self.bed.goal, width),
            engine::shipped(),
        );
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        Sample {
            won: result.won,
            evaluations: result.evaluations,
            ms,
        }
    }
}

fn parse_letters(letters: &[u8]) -> Option<Vec<engine::Piece>> {
    letters
        .iter()
        .map(|letter| {
            engine::pieces::PIECES
                .iter()
                .copied()
                .find(|piece| piece.letter() == *letter)
        })
        .collect()
}

/// Measure the curve and write the artifact. The file is rewritten after every
/// cost point, so a run that is cut short leaves the points it did measure rather
/// than nothing at all — and a partial artifact says so (`complete`, `missing_widths`).
pub fn run(options: &Options) -> Result<Artifact, String> {
    if options.widths.is_empty() {
        return Err("no widths to measure".to_string());
    }
    if options.trials == 0 || options.trial_seeds == 0 || options.quality_games == 0 {
        return Err("trials, trial_seeds and quality_games must all be positive".to_string());
    }
    let mut widths = options.widths.clone();
    widths.sort_unstable();
    if widths.windows(2).any(|window| window[0] == window[1]) {
        return Err("the width list repeats a point".to_string());
    }

    let games: Vec<Games> = Bed::SHIPPED
        .iter()
        .map(|bed| Games::load(*bed, &options.data, TEST_SPLIT))
        .collect::<Result<_, _>>()?;
    for bed in &games {
        let wanted = (TEST_SPLIT.1 - TEST_SPLIT.0 + 1) as usize;
        if bed.len() < wanted {
            return Err(format!(
                "{} has {} test-split games, expected {wanted}",
                bed.bed.name(),
                bed.len()
            ));
        }
    }

    let Identities {
        datasets,
        wasm_bytes,
        wasm_fingerprint,
    } = identities(&options.data)?;

    // ---- phase 1: cost. Ascending, stopping at the first width that fails a budget.
    let mut points: Vec<Point> = Vec::new();
    for width in widths.iter().copied() {
        let mut cost = Vec::new();
        for bed in &games {
            cost.push(measure_cost(bed, width, options)?);
        }
        let row = CostRow {
            width,
            p90_ms: [cost[0].ms_p90, cost[1].ms_p90],
        };
        for bed in &cost {
            say(
                options,
                &format!(
                    "  {:>5}  {:<18} p50 {:8.2} ms  p90 {:8.2} ms  trials [{}]",
                    width,
                    bed.bed,
                    bed.ms_p50,
                    bed.ms_p90,
                    bed.trials_ms_p90
                        .iter()
                        .map(|ms| format!("{ms:.1}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
        }
        points.push(Point {
            width,
            cost,
            quality: None,
            flat_step: None,
        });
        if !row.fits() {
            break;
        }
    }

    // The summary fields are derived once, from the measured points and the probe,
    // against the pre-registered list — the same call `--summarise` makes on a
    // committed file. Deriving them from `options.widths` is exactly the defect this
    // replaces: `--widths 25,…,800` would call a six-point prefix a complete curve
    // and a truncated schedule the thing that stopped it. A run with neither a
    // failing width nor a probe is refused here, before it writes anything.
    let summary = summarise(&points, options.overshoot.as_ref())?;
    let cap = summary.width_cap;

    // A partial artifact, so an interrupted run is still evidence.
    let mut artifact = Artifact {
        generated_at: iso_utc(options.now_secs),
        engine_commit: options.engine_commit.clone(),
        wasm_bytes,
        wasm_fingerprint,
        wasm_path: "web/pkg/engine.wasm".to_string(),
        datasets,
        evaluator_id: "E1".to_string(),
        evaluator_name: "dellacherie-thiery".to_string(),
        evaluator_table: "E1_PUBLISHED".to_string(),
        machine: options.machine.clone(),
        runtime: options.runtime.clone(),
        substrate: match options.ms_source {
            MsSource::Native => "native".to_string(),
            MsSource::Wasm => "wasm".to_string(),
        },
        widths: widths.clone(),
        cost_trials: options.trials,
        cost_seeds_per_trial: options.trial_seeds,
        quality_games: options.quality_games,
        points: points.clone(),
        saturation: Saturation {
            checked_at_width: cap,
            beds: Vec::new(),
            quality_bed: Bed::GOAL16.name(),
            quality_split: TUNING_SPLIT,
            fallback_reason: "pilot not yet run".to_string(),
        },
        quality_axis: QualityAxis::Pending,
        default_beam: cap,
        default_beam_source: DefaultSource::SelfTie,
        default_beam_basis: DefaultBasis::Cost,
        width_cap: summary.width_cap,
        cap_bound: summary.cap_bound,
        overshoot: options.overshoot.clone(),
        quality_fingerprint: quality_fingerprint(&points),
        reason: options.reason.clone(),
        // An in-flight copy is never complete: the phases below have not run. What
        // stopped the *cost* sweep is already known, so it is already on the record.
        complete: false,
        missing_widths: summary.missing_widths.clone(),
        incomplete_reason: if summary.incomplete_reason.is_empty() {
            "in-flight copy written after the cost sweep: §9.1's pilot and the quality series had \
             not run yet"
                .to_string()
        } else {
            format!(
                "in-flight copy written after the cost sweep: §9.1's pilot and the quality series \
                 had not run yet. {}",
                summary.incomplete_reason
            )
        },
        quality_stop_at: None,
    };
    write(&artifact, &options.out)?;

    // ---- phase 2: §9.1's saturation pilot, at the cap, on both shipped beds.
    let mut pilot: Vec<(String, Tally)> = Vec::new();
    for (index, bed) in games.iter().enumerate() {
        let tally = count_wins(bed, cap, options.quality_games);
        say(
            options,
            &format!(
                "  pilot at width {cap}: {} wins {}/{}",
                Bed::SHIPPED[index].name(),
                tally.wins,
                tally.games
            ),
        );
        pilot.push((Bed::SHIPPED[index].name(), tally));
    }
    let saturated = pilot.iter().any(|(_, tally)| tally.saturated());
    let (ranking_bed, ranking_split, fallback_reason) = if saturated {
        (
            Bed::GOAL16,
            TUNING_SPLIT,
            format!(
                "saturation (§9.1): {} at width {cap} on its test split, so the shipped beds cannot rank any width; the validation-only bed goal {}/{}/cap {} over seeds {}-{} ranks them",
                pilot
                    .iter()
                    .filter(|(_, tally)| tally.saturated())
                    .map(|(name, tally)| format!("{name} wins {}/{}", tally.wins, tally.games))
                    .collect::<Vec<_>>()
                    .join(" and "),
                Bed::GOAL16.goal,
                Bed::GOAL16.pieces,
                Bed::GOAL16.cap,
                TUNING_SPLIT.0,
                TUNING_SPLIT.1
            ),
        )
    } else {
        (
            Bed::GOAL12,
            TEST_SPLIT,
            format!(
                "no saturation at width {cap} (§9.1: both shipped beds below {SATURATION_PERCENT}%), so the harder shipped bed ranks the widths on its test split"
            ),
        )
    };
    if ranking_bed != Bed::GOAL12 && ranking_bed != Bed::GOAL16 {
        return Err(
            "internal: the ranking bed must be the harder shipped bed or the validation bed"
                .to_string(),
        );
    }

    // ---- phase 3: the quality sweep on the ranking bed, paired per seed.
    let ranking = Games::load(ranking_bed, &options.data, ranking_split)?;
    let seeds_wanted = options.quality_games as usize;
    if ranking.len() < seeds_wanted {
        return Err(format!(
            "{} has {} games over seeds {}-{}, but {seeds_wanted} are needed",
            ranking_bed.name(),
            ranking.len(),
            ranking_split.0,
            ranking_split.1
        ));
    }
    // The cap's own row first: §9.5's tie is against the cap, so nothing narrower
    // can be judged until the cap has been played.
    let cap_outcomes: Vec<bool> = (0..seeds_wanted)
        .map(|index| ranking.solve(index, cap).won)
        .collect();
    let cap_tally = tally(&cap_outcomes);
    say(
        options,
        &format!(
            "  quality on {} (seeds {}-{}): {cap:>5} wins {}/{}",
            ranking_bed.name(),
            ranking_split.0,
            ranking_split.1,
            cap_tally.wins,
            cap_tally.games
        ),
    );

    let mut measured: Vec<(u32, Vec<bool>)> = Vec::new();
    let mut steps: Vec<Step> = Vec::new();
    let mut quality_stop: Option<u32> = None;
    // Every cost point at or below the cap is played, not just the ones the stop
    // rule needs: §9.5 judges each width against the cap, and a width whose wins
    // were never counted cannot be shown to tie or not to. The stop rule is
    // recorded (`quality_stop_at`) rather than obeyed, so the evidence in the
    // artifact is the whole series and not a prefix of it.
    for width in widths.iter().copied().filter(|width| *width < cap) {
        let outcomes: Vec<bool> = (0..seeds_wanted)
            .map(|index| ranking.solve(index, width).won)
            .collect();
        let here = tally(&outcomes);
        let paired = pair_against(&outcomes, &cap_outcomes);
        say(
            options,
            &format!(
                "  {:>5}  wins {}/{:<5} vs cap b={} c={}  {}",
                width,
                here.wins,
                here.games,
                paired.b,
                paired.c,
                if tie(here, cap_tally, paired) {
                    "ties the cap"
                } else {
                    "does not tie"
                }
            ),
        );
        if let Some((_, previous_outcomes)) = measured.last() {
            // `to` is wider, so `from` is the reference: b counts the seeds the
            // narrower width won and this one lost.
            steps.push((
                here,
                tally(previous_outcomes),
                pair_against(previous_outcomes, &outcomes),
            ));
        }
        measured.push((width, outcomes));
        if quality_stop.is_none() && sweep_is_flat(&steps) {
            quality_stop = Some(width);
            say(
                options,
                &format!(
                    "  §9.5's stop rule would fire after width {width} (two consecutive flat \
                     steps); the series is measured to the cap anyway so every cost point \
                     carries its wins"
                ),
            );
        }
    }
    // The cap's own row is always measured (§9.5's tie is against it), and it is
    // the last row of the ranking list.
    let quality_rows: Vec<QualityRow> = {
        let mut rows: Vec<QualityRow> = measured
            .iter()
            .map(|(width, outcomes)| QualityRow {
                width: *width,
                tally: tally(outcomes),
                paired: pair_against(outcomes, &cap_outcomes),
            })
            .collect();
        rows.push(QualityRow {
            width: cap,
            tally: cap_tally,
            paired: Paired { b: 0, c: 0 },
        });
        rows
    };
    let mut ranked: Vec<(u32, Vec<bool>)> = measured.clone();
    ranked.push((cap, cap_outcomes));

    // ---- phase 4: the two numbers.
    let (default_beam, default_beam_source) = default_beam(&quality_rows);
    let axis = quality_axis(cap_tally);
    let basis = match axis {
        QualityAxis::Resolved => DefaultBasis::Quality,
        QualityAxis::Pending | QualityAxis::NoResolution => DefaultBasis::Cost,
    };

    for point in points.iter_mut() {
        if let Some(row) = quality_rows.iter().find(|row| row.width == point.width) {
            point.quality = Some(*row);
        }
    }
    // Flat steps, for the reader who wants to see the stop rule fire: the step
    // from this width to the next measured quality width, in the same statistic
    // the stop rule uses. No re-solving — the outcomes are already in hand.
    for index in 0..ranked.len().saturating_sub(1) {
        let (width, outcomes_here) = &ranked[index];
        let outcomes_next = &ranked[index + 1].1;
        let is_flat = flat(
            tally(outcomes_next),
            tally(outcomes_here),
            pair_against(outcomes_here, outcomes_next),
        );
        if let Some(point) = points.iter_mut().find(|point| point.width == *width) {
            point.flat_step = Some(is_flat);
        }
    }
    if let Some((last, _)) = ranked.last() {
        if let Some(point) = points.iter_mut().find(|point| point.width == *last) {
            point.flat_step = None;
        }
    }

    artifact.points = points;
    artifact.saturation = Saturation {
        checked_at_width: cap,
        beds: pilot,
        quality_bed: ranking_bed.name(),
        quality_split: ranking_split,
        fallback_reason,
    };
    artifact.quality_axis = axis;
    artifact.default_beam = default_beam;
    artifact.default_beam_source = default_beam_source;
    artifact.default_beam_basis = basis;
    // The derived fields, again and from one place: `complete`, `missing_widths` and
    // `incomplete_reason` are about the pre-registered doubling list, never about the
    // widths this run was asked for, and `cap_bound` is what the evidence says
    // stopped the climb.
    artifact.refresh_summary()?;
    artifact.quality_fingerprint = quality_fingerprint(&artifact.points);
    artifact.quality_stop_at = quality_stop;
    // A curve whose ranking bed could not separate the widths must say so in the
    // one field a reader reads first. A cost decision is never described as a
    // quality one.
    if axis == QualityAxis::NoResolution {
        artifact.reason = format!(
            "{}; §9.1: {} wins {}/{} at width {cap} on seeds {}-{} (≥{SATURATION_PERCENT}%), so the \
             quality axis could not separate the widths and default_beam {default_beam} is the \
             narrowest measured point — named on cost, not on a quality result",
            options.reason.trim_end_matches(['.', ';']),
            ranking_bed.name(),
            cap_tally.wins,
            cap_tally.games,
            ranking_split.0,
            ranking_split.1
        );
    }
    artifact.validate()?;
    write(&artifact, &options.out)?;
    Ok(artifact)
}

fn tally(outcomes: &[bool]) -> Tally {
    Tally {
        wins: outcomes.iter().filter(|won| **won).count() as u32,
        games: outcomes.len() as u32,
    }
}

/// `b` = won in `reference` and lost in `judged`; `c` = the reverse.
fn pair_against(reference: &[bool], judged: &[bool]) -> Paired {
    let mut paired = Paired::default();
    for (reference_won, judged_won) in reference.iter().zip(judged.iter()) {
        match (reference_won, judged_won) {
            (true, false) => paired.b += 1,
            (false, true) => paired.c += 1,
            _ => {}
        }
    }
    paired
}

fn count_wins(games: &Games, width: u32, seeds: u32) -> Tally {
    let mut wins = 0;
    let count = (seeds as usize).min(games.len());
    for index in 0..count {
        if games.solve(index, width).won {
            wins += 1;
        }
    }
    Tally {
        wins,
        games: count as u32,
    }
}

/// One cost point for one bed: `trials` passes over the same `trial_seeds` seeds.
fn measure_cost(games: &Games, width: u32, options: &Options) -> Result<BedCost, String> {
    match options.ms_source {
        MsSource::Native => {
            let mut samples: Vec<Sample> = Vec::new();
            let mut trials_ms_p90 = Vec::new();
            for _trial in 0..options.trials {
                let mut this_trial: Vec<f64> = Vec::new();
                for index in 0..(options.trial_seeds as usize).min(games.len()) {
                    let sample = games.solve(index, width);
                    this_trial.push(sample.ms);
                    samples.push(sample);
                }
                trials_ms_p90.push(p90(&this_trial));
            }
            Ok(assemble(games.bed, samples, trials_ms_p90))
        }
        MsSource::Wasm => {
            let rows = wasm_samples(games.bed, width, options)?;
            let mut samples = Vec::new();
            let mut trials_ms_p90 = Vec::new();
            for trial in rows {
                trials_ms_p90.push(p90(&trial
                    .iter()
                    .map(|sample| sample.ms)
                    .collect::<Vec<_>>()));
                samples.extend_from_slice(&trial);
            }
            Ok(assemble(games.bed, samples, trials_ms_p90))
        }
    }
}

fn assemble(bed: Bed, samples: Vec<Sample>, trials_ms_p90: Vec<f64>) -> BedCost {
    let ms: Vec<f64> = samples.iter().map(|sample| sample.ms).collect();
    let evaluations: Vec<f64> = samples
        .iter()
        .map(|sample| sample.evaluations as f64)
        .collect();
    BedCost {
        bed: bed.name(),
        matches: Tally {
            wins: samples.iter().filter(|sample| sample.won).count() as u32,
            games: samples.len() as u32,
        },
        evaluations_p50: nearest_rank(
            &{
                let mut sorted = evaluations;
                sorted.sort_by(|a, b| a.partial_cmp(b).expect("no NaN"));
                sorted
            },
            1,
            2,
        ) as u32,
        ms_p50: median(&ms),
        ms_p90: p90(&ms),
        ms_max: ms.iter().copied().fold(0.0, f64::max),
        trials_ms_p90,
    }
}

/// Ask the built wasm artifact, through the page's own loader, for `trials`
/// passes of `trial_seeds` solves. One node process per (bed, width): the wasm
/// compile is 20 ms against minutes of solving, and a fresh process keeps a slow
/// trial from being charged to its neighbour.
fn wasm_samples(bed: Bed, width: u32, options: &Options) -> Result<Vec<Vec<Sample>>, String> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tools")
        .join("wasm-timing.mjs");
    if !script.exists() {
        return Err(format!("missing {}", script.display()));
    }
    let output = Command::new("node")
        .arg(&script)
        .arg("--goal")
        .arg(bed.goal.to_string())
        .arg("--pieces")
        .arg(bed.pieces.to_string())
        .arg("--cap")
        .arg(bed.cap.to_string())
        .arg("--width")
        .arg(width.to_string())
        .arg("--trials")
        .arg(options.trials.to_string())
        .arg("--seeds")
        .arg(options.trial_seeds.to_string())
        .output()
        .map_err(|error| format!("run {}: {error}", script.display()))?;
    if !output.status.success() {
        return Err(format!(
            "{} failed: {}",
            script.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut trials: Vec<Vec<Sample>> = vec![Vec::new(); options.trials as usize];
    for text in stdout.lines() {
        // `trial,index,won,evaluations,ms`
        if text.starts_with('#') || text.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = text.split(',').collect();
        if fields.len() != 5 {
            return Err(format!("wasm-timing: unexpected row {text:?}"));
        }
        let trial: usize = fields[0]
            .parse()
            .map_err(|_| format!("wasm-timing: trial {:?}", fields[0]))?;
        let won = match fields[2] {
            "1" | "true" => true,
            "0" | "false" => false,
            other => return Err(format!("wasm-timing: won {other:?}")),
        };
        let evaluations: u32 = fields[3]
            .parse()
            .map_err(|_| format!("wasm-timing: evaluations {:?}", fields[3]))?;
        let ms: f64 = fields[4]
            .parse()
            .map_err(|_| format!("wasm-timing: ms {:?}", fields[4]))?;
        if trial >= trials.len() {
            return Err(format!("wasm-timing: trial {trial} out of range"));
        }
        trials[trial].push(Sample {
            won,
            evaluations,
            ms,
        });
    }
    if trials.iter().any(|trial| trial.is_empty()) {
        return Err("wasm-timing: a trial produced no samples".to_string());
    }
    Ok(trials)
}

/// The two committed identities a re-run must reproduce: the Dataset
/// fingerprints (§6.1) and the wasm artifact the page will load.
struct Identities {
    /// `(config dir, fingerprint, generator_version)` per shipped bed.
    datasets: Vec<(String, String, u32)>,
    wasm_bytes: u64,
    wasm_fingerprint: String,
}

fn identities(data: &Path) -> Result<Identities, String> {
    let mut datasets = Vec::new();
    for config in dataset::CONFIGS {
        let path = dataset::manifest_path(data, &config);
        let text = fs::read_to_string(&path)
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        let manifest = dataset::Manifest::parse(&text)
            .map_err(|error| format!("parse {}: {error}", path.display()))?;
        datasets.push((
            config.dir.to_string(),
            manifest.fingerprint.clone(),
            manifest.generator_version,
        ));
    }
    let wasm = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("bench/ has a parent")
        .join("web")
        .join("pkg")
        .join("engine.wasm");
    match fs::read(&wasm) {
        Ok(bytes) => {
            let mut hash = Fnv::new();
            for byte in &bytes {
                hash.byte(*byte);
            }
            Ok(Identities {
                datasets,
                wasm_bytes: bytes.len() as u64,
                wasm_fingerprint: format!("fnv1a64:{}", fingerprint::hex(hash.finish())),
            })
        }
        Err(_) => Ok(Identities {
            datasets,
            wasm_bytes: 0,
            wasm_fingerprint: "missing: web/pkg/engine.wasm was not built".to_string(),
        }),
    }
}

fn write(artifact: &Artifact, out: &Path) -> Result<(), String> {
    write_text(&artifact.render(), out)
}

fn write_text(text: &str, out: &Path) -> Result<(), String> {
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create {}: {error}", parent.display()))?;
        }
    }
    let mut file =
        fs::File::create(out).map_err(|error| format!("write {}: {error}", out.display()))?;
    file.write_all(text.as_bytes())
        .map_err(|error| format!("write {}: {error}", out.display()))?;
    Ok(())
}

fn say(options: &Options, message: &str) {
    if !options.quiet {
        println!("{message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_big_integer_multiplies_and_divides_exactly() {
        let mut value = Big::zero();
        value.add_small(1);
        for k in 1..=30u32 {
            value.mul_small(k as u64 + 5);
            assert_eq!(value.div_small(k as u64 + 5), 0);
        }
        let mut power = Big::pow2(64);
        power.div_small(2);
        assert_eq!(power, Big::pow2(63));
        assert_eq!(
            Big::pow2(0).cmp_big(&Big::zero()),
            std::cmp::Ordering::Greater
        );
    }
}
