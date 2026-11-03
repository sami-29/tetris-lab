//! Ticket 08: the ablation ladder that names the shipped Evaluator (§9.4).
//!
//! Four questions, in the order the spec fixes, and each one is a decision
//! procedure rather than an opinion:
//!
//! 1. **Is the bed saturated?** §9.1's check, at the width cap, bought from
//!    [`crate::width`]'s artifact rather than re-measured: both shipped beds win
//!    1000/1000 at the cap, and so does §9.1's validation bed. No bed the spec
//!    pre-registers can rank an evaluator.
//! 2. **So the bed is derived, and says so.** The operator's run decision
//!    (notes/08, "Run decision"): at [`SCAN_PIECES`] pieces and cap [`SCAN_CAP`],
//!    scan goals upward on the tuning Split until the *weakest* Evaluator (E0)
//!    wins fewer than 99 % of games. That goal is the ladder's bed
//!    ([`derive_bed`]). If no goal does it, this module refuses instead of
//!    inventing a bed §5.8 forbids.
//! 3. **The pre-registered statistics, unchanged** ([`compare`], [`cascade`]):
//!    paired per seed, exact McNemar on win rate — `p >= 0.01` is a tie — then
//!    fewer Evaluations per Solve, then fewer Features, then §9.4's own closing
//!    sentence: the published table ships over a tuned one.
//! 4. **One-at-a-time toggles off the winner** ([`toggle_verdict`]): a switch that
//!    moves nothing beyond its interval is deleted from the engine, not kept.
//!
//! Two numbers are pinned so a re-run is a re-reading and not a new experiment:
//! the bootstrap's stream is [`bootstrap_seed`], a function of the artifact's own
//! fields, and the per-seed rows are committed next to the summary, because
//! §9.4's statistics must stay recomputable after the losing architecture is
//! deleted from `engine/`.
//!
//! Nothing here touches the engine's semantics: the Evaluator is chosen by a
//! `match` in `bench/`, never by a field in [`engine::SolveConfig`], the URL or
//! the ABI (§2.2, §4.2).

use std::path::{Path, PathBuf};
use std::time::Instant;

use engine::search::{Feature, Toggles};
use engine::{DellacherieThiery, Evaluator, Heuristic};

use crate::fingerprint::{self, Fnv};
use crate::tuning::stats::{self, Sample, Stats};
use crate::width::{self, Bed, Paired};

// ------------------------------------------------------------------ constants

/// The scan's piece count: §9.1's validation bed.
pub const SCAN_PIECES: u32 = 160;

/// The scan's height cap: §9.1's validation bed.
pub const SCAN_CAP: u32 = 7;

/// The scan starts at the goal §9.1 already fixed (16), so the first row is the
/// bed the spec pre-registers and every later row is the climb above it.
pub const SCAN_FIRST_GOAL: u32 = width::VALIDATION_GOAL;

/// §9.1's threshold, as an integer: `wins * 100 >= games * 99` is saturated.
pub const SATURATION_PERCENT: u32 = width::SATURATION_PERCENT;

/// §9.4's resample count. Pinned, not a flag: the interval is a function of this
/// and of [`bootstrap_seed`], and of nothing else.
pub const BOOTSTRAP_RESAMPLES: u32 = 10_000;

/// The artifact's schema version.
pub const VERSION: u32 = 1;

/// The six rungs the artifact carries, in the order the table publishes them.
pub const ROWS: [&'static str; 6] = [
    "e0",
    "e1",
    "e2",
    "dedup-off",
    "tail-off",
    "no-suicide-off",
];

/// The three Evaluators the ladder compares, in §9.4's order.
pub const EVALUATORS: [&'static str; 3] = ["e0", "e1", "e2"];

// ---------------------------------------------------------------- the goal scan

/// One goal's E0 row: the count that decides whether the bed can rank anything,
/// plus two context numbers a reader needs to see that the game really was
/// played (mean Lines and mean Evaluations).
#[derive(Clone, Debug, PartialEq)]
pub struct ScanRow {
    pub goal: u32,
    pub wins: u32,
    pub games: u32,
    pub mean_lines: f64,
    pub evals_mean: f64,
}

impl ScanRow {
    /// §9.1, exactly: `>= 99 %` is saturated and cannot rank.
    pub fn saturated(&self) -> bool {
        self.games > 0
            && (self.wins as u64) * 100 >= (self.games as u64) * (SATURATION_PERCENT as u64)
    }
}

/// The index of the first goal whose E0 win rate is below 99 %, scanning in the
/// order the rows were measured (upward).
pub fn first_goal_below_saturation(rows: &[ScanRow]) -> Option<usize> {
    rows.iter().position(|row| !row.saturated())
}

/// The scan stops once two goals in a row were reached by nobody: above that line
/// the board cannot be filled at all, so there is nothing left to learn.
pub fn scan_is_done(rows: &[ScanRow]) -> bool {
    rows.len() >= 2 && rows[rows.len() - 1].wins == 0 && rows[rows.len() - 2].wins == 0
}

/// The first goal of the trailing run of goals nobody reached, e.g. `69` for the
/// 64-line ceiling at 160 pieces (the §5.7 pre-fill is what lifts it).
pub fn unreachable_from(rows: &[ScanRow]) -> Option<u32> {
    let mut start = None;
    for row in rows.iter().rev() {
        if row.wins == 0 {
            start = Some(row.goal);
        } else {
            break;
        }
    }
    start
}

/// The most Lines `pieces` pieces can clear: four cells each, ten cells a row.
/// The number the protocol's "64-line ceiling" came from — and the reason goal 65
/// needed a check rather than an assumption, since the §5.7 pre-fill puts cells on
/// the Board before the first piece.
pub fn line_ceiling(pieces: u32) -> u32 {
    pieces.saturating_mul(4) / 10
}

/// The bed the ladder runs on: the first sub-99 % goal, or a refusal.
///
/// This is the operator's run decision as code, and the refusal is half of it:
/// "if no goal within the Domain produces < 99 %, stop and report rather than
/// inventing a bed the Domain forbids".
pub fn derive_bed(rows: &[ScanRow], pieces: u32, cap: u32) -> Result<Bed, String> {
    match first_goal_below_saturation(rows) {
        Some(index) => Ok(Bed {
            goal: rows[index].goal,
            pieces,
            cap,
        }),
        None => {
            let first = rows.first().map(|row| row.goal).unwrap_or(SCAN_FIRST_GOAL);
            let last = rows.last().map(|row| row.goal).unwrap_or(SCAN_FIRST_GOAL);
            Err(format!(
                "no goal at {pieces} pieces and cap {cap} drops E0 below {SATURATION_PERCENT} % \
                 (scanned {first}..={last}); the bed is not pre-registered and will not be invented"
            ))
        }
    }
}

// ------------------------------------------------------------------- evaluators

/// One rung: an Evaluator and the search decisions it ran with, measured on one
/// Split.
#[derive(Clone, Debug)]
pub struct Candidate {
    /// `e0`, `e1`, `e2`, `dedup-off`, `tail-off`, `no-suicide-off`.
    pub id: &'static str,
    /// The committed table's name, for the table's first column.
    pub table: &'static str,
    /// §9.4's last tie-break: the number of Features the Evaluator measures, not
    /// the number of non-zero weights (E1 has two zero weights and still measures
    /// eight).
    pub features: u32,
    /// Whether the table is published rather than tuned: §9.4's closing sentence.
    pub published: bool,
    /// The implementation type, not the table: `heuristic` or `dellacherie-thiery`.
    /// §9.4's closing sentence only ever separates two *tables of one
    /// architecture* (E2 is a config swap on E1), so the rule that drops a tuned
    /// table in favour of a published one is scoped by this field and cannot
    /// silently retire a different architecture's control.
    pub architecture: &'static str,
    pub samples: Vec<Sample>,
}

impl Candidate {
    pub fn stats(&self) -> Stats {
        stats::summarise(&self.samples)
    }
}

// ---------------------------------------------------------------- the statistics

/// One paired comparison, `judged` against `reference`, both over the same seeds.
///
/// The 2×2 table is the artifact's evidence, not the p-value: `only_judged` and
/// `only_reference` are the discordant seeds and their difference is exactly the
/// win-count difference, so a reader can check the arithmetic without the big
/// integers.
#[derive(Clone, Debug, PartialEq)]
pub struct Pair {
    /// The row being judged, e.g. `e0`.
    pub judged: &'static str,
    /// The row it is judged against, e.g. the winner.
    pub reference: &'static str,
    pub both: u32,
    /// `judged` wins, `reference` loses.
    pub only_judged: u32,
    /// `reference` wins, `judged` loses.
    pub only_reference: u32,
    pub neither: u32,
    /// §9.5's exact two-sided sign test on the discordant pairs.
    pub discordant: Paired,
    /// `p < 0.01`: the difference is real. `false` is §9.4's tie.
    pub rejects: bool,
    /// Mean `lines_judged - lines_reference` over every seed, failures included.
    pub delta_mean_lines: f64,
    /// The paired bootstrap 95 % interval of the same difference.
    pub ci95: [f64; 2],
    /// The stream the resamples were drawn from: a function of the artifact's own
    /// fields, so a re-run reproduces the interval.
    pub boot_seed: u64,
    pub evals_median_judged: f64,
    pub evals_median_reference: f64,
    /// Seeds whose evaluation count differs at all: how much of the evals leg is
    /// actually exercised, which a bare median difference cannot show.
    pub evals_differ_seeds: u32,
    /// Seeds where any of `(won, lines, evals)` differs.
    pub moved_seeds: u32,
}

impl Pair {
    /// The interval excludes zero, i.e. the mean-lines axis resolves the pair.
    pub fn ci95_excludes_zero(&self) -> bool {
        self.ci95[1] < 0.0 || self.ci95[0] > 0.0
    }

    /// The 2×2 table's cells sum to the game count.
    pub fn games(&self) -> u32 {
        self.both + self.only_judged + self.only_reference + self.neither
    }
}

/// The paired 2×2 counts of `judged` against `reference`.
fn two_by_two(judged: &[Sample], reference: &[Sample]) -> (u32, u32, u32, u32) {
    assert_eq!(
        judged.len(),
        reference.len(),
        "a pairing needs the same seeds"
    );
    let (mut both, mut only_judged, mut only_reference, mut neither) = (0, 0, 0, 0);
    for (a, b) in judged.iter().zip(reference) {
        assert_eq!(a.seed, b.seed, "the rows are not paired by seed");
        match (a.won, b.won) {
            (true, true) => both += 1,
            (true, false) => only_judged += 1,
            (false, true) => only_reference += 1,
            (false, false) => neither += 1,
        }
    }
    (both, only_judged, only_reference, neither)
}

/// Compare one rung against another, paired per seed, with the bootstrap's stream
/// given (see [`bootstrap_seed`]).
pub fn compare(judged: &Candidate, reference: &Candidate, boot_seed: u64) -> Pair {
    let (both, only_judged, only_reference, neither) =
        two_by_two(&judged.samples, &reference.samples);
    let discordant = Paired {
        b: only_judged,
        c: only_reference,
    };
    let judged_stats = judged.stats();
    let reference_stats = reference.stats();
    let moved_seeds = judged
        .samples
        .iter()
        .zip(&reference.samples)
        .filter(|(a, b)| a.won != b.won || a.lines != b.lines || a.evals != b.evals)
        .count() as u32;
    let evals_differ_seeds = judged
        .samples
        .iter()
        .zip(&reference.samples)
        .filter(|(a, b)| a.evals != b.evals)
        .count() as u32;
    Pair {
        judged: judged.id,
        reference: reference.id,
        both,
        only_judged,
        only_reference,
        neither,
        discordant,
        rejects: width::mcnemar_rejects(discordant),
        delta_mean_lines: judged_stats.mean_lines - reference_stats.mean_lines,
        ci95: stats::paired_bootstrap_ci95(
            &reference.samples,
            &judged.samples,
            boot_seed,
            BOOTSTRAP_RESAMPLES,
        ),
        boot_seed,
        evals_median_judged: judged_stats.evals_median,
        evals_median_reference: reference_stats.evals_median,
        evals_differ_seeds,
        moved_seeds,
    }
}

// ------------------------------------------------------------------- the cascade

/// Which leg of §9.4's cascade decided the winner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leg {
    /// Highest win rate, and the difference cleared `p < 0.01`.
    WinRate,
    /// A win-rate tie, broken by Evaluations per Solve.
    Evaluations,
    /// Both tied, broken by the number of Features.
    Features,
    /// §9.4's closing sentence: the published table ships over a tuned one of the
    /// same architecture unless the tuned one clears `p < 0.01` on win rate. It is
    /// applied before the legs, so it is the reason the tuned table is gone rather
    /// than a leg the tie survived to.
    Published,
}

impl Leg {
    pub fn as_str(self) -> &'static str {
        match self {
            Leg::WinRate => "win rate",
            Leg::Evaluations => "fewer evaluations per solve",
            Leg::Features => "fewer features",
            Leg::Published => "the published table",
        }
    }
}

/// What the ladder decided, and how.
#[derive(Clone, Debug, PartialEq)]
pub struct Verdict {
    pub winner: &'static str,
    pub leg: Leg,
    /// The rungs that were still standing when the legs started: every rung whose
    /// difference from the highest win rate was `p >= 0.01`.
    pub tied: Vec<&'static str>,
    /// One line per leg that ran, so the artifact reads as a decision procedure
    /// rather than a conclusion.
    pub steps: Vec<String>,
}

/// §9.4's cascade, over rows that are already measured.
///
/// Legs, in the spec's own order: highest win rate (a difference with
/// `p >= 0.01` is a tie) → fewer Evaluations per Solve → fewer Features → the
/// published table. The last leg is §9.4's closing sentence, "E1 ships unless E2
/// clears `p < 0.01`", read as the tie-break it is.
pub fn cascade(candidates: &[Candidate]) -> Result<Verdict, String> {
    if candidates.is_empty() {
        return Err("the ladder has no rows to rank".to_string());
    }
    let win_counts: Vec<u32> = candidates.iter().map(|c| c.stats().wins).collect();
    let top = win_counts
        .iter()
        .enumerate()
        .fold(0usize, |best, (index, wins)| {
            if *wins > win_counts[best] {
                index
            } else {
                best
            }
        });
    let mut steps = vec![format!(
        "highest win rate: {} ({} of {})",
        candidates[top].id,
        win_counts[top],
        candidates[top].stats().games
    )];

    let mut tied: Vec<&Candidate> = vec![&candidates[top]];
    for (index, candidate) in candidates.iter().enumerate() {
        if index == top {
            continue;
        }
        let (_, only_candidate, only_winner, _) =
            two_by_two(&candidate.samples, &candidates[top].samples);
        let discordant = Paired {
            b: only_candidate,
            c: only_winner,
        };
        let rejects = width::mcnemar_rejects(discordant);
        steps.push(format!(
            "{} vs {}: b = {}, c = {}, n = {} -> {}",
            candidate.id,
            candidates[top].id,
            only_candidate,
            only_winner,
            discordant.n(),
            if rejects {
                "different at p < 0.01, out"
            } else {
                "p >= 0.01, a tie"
            }
        ));
        if !rejects {
            tied.push(candidate);
        }
    }
    let tied_ids: Vec<&'static str> = tied.iter().map(|candidate| candidate.id).collect();
    if tied.len() == 1 {
        return Ok(Verdict {
            winner: candidates[top].id,
            leg: Leg::WinRate,
            tied: tied_ids,
            steps,
        });
    }

    // §9.4's closing sentence, applied first because it fixes one pair before any
    // leg can: "E1 ships unless E2 clears p < 0.01". A tuned table only displaces
    // a published one of the *same architecture* by winning on win rate; a 24-
    // evaluation median difference out of ninety thousand does not carry it. The
    // evals numbers are still recorded by the leg below, so a reader can see that
    // this rule, and not a measurement, is what kept the published table.
    let mut tuned: Vec<&'static str> = Vec::new();
    for published in tied.iter().filter(|candidate| candidate.published) {
        for candidate in tied.iter().copied() {
            if candidate.published
                || candidate.architecture != published.architecture
                || tuned.contains(&candidate.id)
            {
                continue;
            }
            let (_, only_tuned, only_published, _) =
                two_by_two(&candidate.samples, &published.samples);
            let discordant = Paired {
                b: only_tuned,
                c: only_published,
            };
            if !width::mcnemar_rejects(discordant) {
                tuned.push(candidate.id);
                steps.push(format!(
                    "{} vs {}: b = {}, c = {}, n = {} -> the tuned table does not clear \
                     p < 0.01, so {} ships (§9.4)",
                    candidate.id,
                    published.id,
                    only_tuned,
                    only_published,
                    discordant.n(),
                    published.id
                ));
            }
        }
    }
    if !tuned.is_empty() {
        tied.retain(|candidate| !tuned.contains(&candidate.id));
        if tied.len() == 1 {
            return Ok(Verdict {
                winner: tied[0].id,
                leg: Leg::Published,
                tied: tied_ids,
                steps,
            });
        }
    }

    // Leg 1: fewer Evaluations per Solve. The median first, because §9.4 says "per
    // solve" and the median is what one game's cost is.
    let evals = |candidate: &Candidate| candidate.stats().evals_median;
    let fewest = tied
        .iter()
        .map(|candidate| evals(candidate))
        .fold(f64::INFINITY, f64::min);
    let kept: Vec<&Candidate> = tied
        .iter()
        .copied()
        .filter(|candidate| evals(candidate) == fewest)
        .collect();
    steps.push(format!(
        "evaluations per solve (median): {} -> kept {}",
        tied
            .iter()
            .map(|candidate| format!("{} {}", candidate.id, evals(candidate)))
            .collect::<Vec<_>>()
            .join(", "),
        kept.iter()
            .map(|candidate| candidate.id)
            .collect::<Vec<_>>()
            .join(", ")
    ));
    if kept.is_empty() {
        return Err("the evaluations leg kept nothing".to_string());
    }
    if kept.len() == 1 {
        return Ok(Verdict {
            winner: kept[0].id,
            leg: Leg::Evaluations,
            tied: tied_ids,
            steps,
        });
    }

    // Leg 2: fewer Features — the count of Features measured, never the count of
    // non-zero weights.
    let fewest = kept
        .iter()
        .map(|candidate| candidate.features)
        .min()
        .unwrap_or(0);
    let narrow: Vec<&Candidate> = kept
        .iter()
        .copied()
        .filter(|candidate| candidate.features == fewest)
        .collect();
    steps.push(format!(
        "features: {} -> kept {}",
        kept.iter()
            .map(|candidate| format!("{} {}", candidate.id, candidate.features))
            .collect::<Vec<_>>()
            .join(", "),
        narrow
            .iter()
            .map(|candidate| candidate.id)
            .collect::<Vec<_>>()
            .join(", ")
    ));
    if narrow.len() == 1 {
        return Ok(Verdict {
            winner: narrow[0].id,
            leg: Leg::Features,
            tied: tied_ids,
            steps,
        });
    }

    // Leg 3: §9.4's own closing sentence. Between a published table and a tuned
    // one at a tie, the published one ships.
    let published: Vec<&Candidate> = narrow
        .iter()
        .copied()
        .filter(|candidate| candidate.published)
        .collect();
    steps.push(format!(
        "published over tuned: kept {}",
        published
            .iter()
            .map(|candidate| candidate.id)
            .collect::<Vec<_>>()
            .join(", ")
    ));
    match published.as_slice() {
        [one] => Ok(Verdict {
            winner: one.id,
            leg: Leg::Published,
            tied: tied_ids,
            steps,
        }),
        _ => Err(format!(
            "the cascade cannot separate {}: same win rate, same evaluations, same feature count \
             and the same publication status",
            narrow
                .iter()
                .map(|candidate| candidate.id)
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The bootstrap's stream: FNV-1a-64 over the artifact's own fields, so any reader
/// can recompute the interval without knowing anything private, and a re-run of the
/// same measurement is bit-identical.
///
/// A different pair name, width, bed or Dataset fingerprint gives a different
/// stream on purpose: that is a new measurement, not a new reading of an old one.
pub fn bootstrap_seed(bed: &str, width: u32, pair: &str, dataset_fingerprint: &str) -> u64 {
    let mut fnv = Fnv::new();
    let text =
        format!("ladder-bootstrap/{VERSION}|{bed}|{width}|{pair}|dfp={dataset_fingerprint}");
    for byte in text.bytes() {
        fnv.byte(byte);
    }
    fnv.finish()
}

// ------------------------------------------------------------------ the toggles

/// What a one-at-a-time toggle earned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToggleVerdict {
    /// Kept: it moved something beyond its interval, so it stays in the engine.
    Kept,
    /// Deleted from the engine, not flagged off (§9.4).
    Deleted,
}

impl ToggleVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            ToggleVerdict::Kept => "kept",
            ToggleVerdict::Deleted => "deleted",
        }
    }
}

/// §9.4's deletion rule, made mechanical: "a toggle that moves nothing beyond its
/// CI is deleted from the engine, not kept because it might help".
///
/// Two statistics, because one is not enough (§9.5's own lesson): the paired
/// mean-Lines interval, and the exact McNemar test on the seeds whose outcome
/// flipped. A mechanism that never engaged changed not one number on the Split and
/// has nothing to keep; a mechanism that engaged and moved nothing is deleted with
/// its counts recorded next to it.
pub fn toggle_verdict(engaged: bool, moved: u32, discordant: Paired, ci95: [f64; 2]) -> ToggleVerdict {
    if !engaged || moved == 0 {
        return ToggleVerdict::Deleted;
    }
    if width::mcnemar_rejects(discordant) || ci95[1] < 0.0 || ci95[0] > 0.0 {
        return ToggleVerdict::Kept;
    }
    ToggleVerdict::Deleted
}

// ------------------------------------------------------------------ the artifact

/// One rung as the artifact records it: the summary a reader quotes and the
/// per-seed rows the statistics are recomputable from.
#[derive(Clone, Debug)]
pub struct RungRow {
    pub id: &'static str,
    pub table: &'static str,
    pub features: u32,
    pub toggles: String,
    pub stats: Stats,
    pub samples: Vec<Sample>,
}

/// §9.1's check as the artifact quotes it: read from the width curve's own pilot.
#[derive(Clone, Debug, PartialEq)]
pub struct SatSummary {
    pub checked_at_width: u32,
    pub beds: Vec<(String, u32, u32)>,
    pub source: String,
}

/// Everything one ladder run measured.
#[derive(Clone, Debug)]
pub struct Artifact {
    pub date: String,
    pub engine_commit: String,
    pub machine: String,
    pub bed: Bed,
    pub bed_source: String,
    pub scan: Vec<ScanRow>,
    pub scan_pieces: u32,
    pub scan_cap: u32,
    pub split: (u32, u32),
    pub width: u32,
    pub width_cap: u32,
    pub quality_fingerprint: String,
    pub bed_fingerprint: String,
    pub rows: Vec<RungRow>,
    /// Every rung that is not the winner, against the winner.
    pub pairs: Vec<Pair>,
    pub verdict: Verdict,
    /// `(toggle ids, verdict, the pair that decided it)`, in `ROWS[3..]` order.
    pub toggles: Vec<ToggleVerdict>,
    pub toggle_pairs: Vec<Pair>,
    pub saturation: SatSummary,
}

impl Artifact {
    /// One rung by id.
    pub fn row(&self, id: &str) -> &RungRow {
        self.rows
            .iter()
            .find(|row| row.id == id)
            .unwrap_or_else(|| panic!("the artifact has no row {id}"))
    }

    /// The pair that judged `id` against the winner, if there is one.
    pub fn pair(&self, id: &str) -> Option<&Pair> {
        self.pairs
            .iter()
            .chain(self.toggle_pairs.iter())
            .find(|pair| pair.judged == id)
    }

    /// The toggles §9.4's rule deleted.
    pub fn deleted_toggles(&self) -> Vec<&'static str> {
        self.toggle_pairs
            .iter()
            .zip(&self.toggles)
            .filter(|(_, verdict)| **verdict == ToggleVerdict::Deleted)
            .map(|(pair, _)| pair.judged)
            .collect()
    }
}

/// `bench ladder`'s options.
#[derive(Clone, Debug)]
pub struct Options {
    pub out: PathBuf,
    pub rows_path: PathBuf,
    pub docs_path: PathBuf,
    /// The bed to run on. `None` derives it by the scan, which is the shipped path:
    /// a bed handed to the ladder is not the one the protocol names, so the
    /// artifact records `bed_source: "given"`.
    pub bed: Option<Bed>,
    pub seeds: (u32, u32),
    pub width: u32,
    pub width_cap: u32,
    pub quality_fingerprint: String,
    pub machine: String,
    pub engine_commit: String,
    pub date: String,
    pub scan_pieces: u32,
    pub scan_cap: u32,
    pub saturation: SatSummary,
}

impl Options {
    pub fn defaults() -> Options {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("bench/ has a parent")
            .to_path_buf();
        Options {
            out: root.join("data/measurements/ablation-ladder.json"),
            rows_path: root.join("data/measurements/ablation-ladder-rows.jsonl"),
            docs_path: root.join("docs/measurements/ablation-ladder.md"),
            bed: None,
            seeds: (width::TUNING_SPLIT.0, width::TUNING_SPLIT.1),
            width: 25,
            width_cap: 800,
            quality_fingerprint: "unrecorded".to_string(),
            machine: "unrecorded".to_string(),
            engine_commit: "unrecorded".to_string(),
            date: "unrecorded".to_string(),
            scan_pieces: SCAN_PIECES,
            scan_cap: SCAN_CAP,
            saturation: SatSummary {
                checked_at_width: 0,
                beds: Vec::new(),
                source: "unrecorded".to_string(),
            },
        }
    }
}

// -------------------------------------------------------------- the measurement

/// One generated bed, held flat so the worker threads share it without a lock.
struct Games {
    seeds: Vec<u32>,
    boards: Vec<engine::Board>,
    sequences: Vec<Vec<engine::Piece>>,
}

impl Games {
    fn new(bed: Bed, split: (u32, u32)) -> Result<Games, String> {
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
            seeds,
            boards,
            sequences,
        })
    }

    fn len(&self) -> usize {
        self.seeds.len()
    }

    /// §6.1's semantic fingerprint over games that are generated rather than
    /// committed: the seed, the ten column words and the sequence letters, in seed
    /// order.
    fn fingerprint(&self) -> String {
        let mut fnv = Fnv::new();
        for index in 0..self.len() {
            fnv.u32_le(self.seeds[index]);
            for word in self.boards[index].0 {
                fnv.u32_le(word);
            }
            for piece in &self.sequences[index] {
                fnv.byte(piece.letter());
            }
        }
        fingerprint::hex(fnv.finish())
    }
}

/// Which Evaluator a rung scores with, as a `match` in the bench and nothing more.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Table {
    E0,
    E1,
    E2,
}

impl Table {
    /// `(table, committed table name, features, published, architecture)`.
    fn for_id(id: &str) -> Result<(Table, &'static str, u32, bool, &'static str), String> {
        match id {
            "e0" => Ok((Table::E0, "E0_HEURISTIC", 4, false, "heuristic")),
            "e1" => Ok((Table::E1, "E1_PUBLISHED", 8, true, "dellacherie-thiery")),
            "e2" => Ok((Table::E2, "E2_TUNED", 8, false, "dellacherie-thiery")),
            other => Err(format!(
                "{other} is not an Evaluator row; the ladder's are {}",
                EVALUATORS.join(", ")
            )),
        }
    }

    fn evaluator(self) -> &'static dyn Evaluator {
        match self {
            // The control is named explicitly, so a §9.4 verdict can never move the
            // control's own numbers.
            Table::E0 => &CONTROL,
            Table::E1 => &PUBLISHED,
            Table::E2 => &TUNED,
        }
    }
}

static CONTROL: Heuristic = Heuristic(engine::E0_HEURISTIC);
static PUBLISHED: DellacherieThiery = DellacherieThiery(engine::E1_PUBLISHED);
static TUNED: DellacherieThiery = DellacherieThiery(engine::E2_TUNED);

/// Which search decision a toggle row switches off.
fn toggle_for_id(id: &str) -> Result<Feature, String> {
    match id {
        "dedup-off" => Ok(Feature::Dedup),
        "tail-off" => Ok(Feature::Tail),
        "no-suicide-off" => Ok(Feature::NoSuicide),
        other => Err(format!("{other} is not one of the ladder's toggle rows")),
    }
}

fn toggles_label(toggles: Toggles) -> String {
    let mut parts = Vec::new();
    if toggles.dedup {
        parts.push("dedup");
    }
    if toggles.tail {
        parts.push("tail");
    }
    if toggles.no_suicide {
        parts.push("no-suicide");
    }
    if parts.is_empty() {
        "none".to_string()
    } else {
        parts.join("+")
    }
}

/// `f(0..n)` across the machine's cores, returned in index order.
///
/// Each game is independent — the search has no shared state and the engine has no
/// RNG — so the split is deterministic even though the scheduling is not, and every
/// reduction afterwards runs on the main thread in seed order.
fn map_parallel(n: usize, f: &(impl Fn(usize) -> Sample + Sync)) -> Vec<Sample> {
    let threads = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .min(n.max(1));
    if threads <= 1 || n < 4 {
        return (0..n).map(f).collect();
    }
    let chunk = n.div_ceil(threads);
    let mut parts: Vec<Vec<Sample>> = Vec::new();
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for start in (0..n).step_by(chunk) {
            let end = (start + chunk).min(n);
            handles.push(scope.spawn(move || (start..end).map(f).collect::<Vec<Sample>>()));
        }
        for handle in handles {
            parts.push(handle.join().expect("a worker thread panicked"));
        }
    });
    parts.into_iter().flatten().collect()
}

/// One rung: every game of the Split, solved once, in seed order.
fn measure(games: &Games, bed: Bed, width: u32, table: Table, toggles: Toggles) -> Vec<Sample> {
    map_parallel(games.len(), &|index| {
        let cfg = engine::SolveConfig {
            goal: bed.goal,
            beam_width: width,
            toggles,
        };
        let started = Instant::now();
        let result = engine::solve(
            &games.boards[index],
            &games.sequences[index],
            &cfg,
            table.evaluator(),
        );
        Sample {
            seed: games.seeds[index],
            won: result.won,
            lines: result.lines,
            evals: result.evaluations,
            moves: result.moves.len() as u32,
            ms: started.elapsed().as_secs_f64() * 1000.0,
            pieces: bed.pieces,
        }
    })
}

/// The measured rung as the artifact stores it.
fn rung(
    games: &Games,
    bed: Bed,
    width: u32,
    id: &'static str,
    table: Table,
    table_name: &'static str,
    features: u32,
    toggles: Toggles,
) -> RungRow {
    let samples = measure(games, bed, width, table, toggles);
    RungRow {
        id,
        table: table_name,
        features,
        toggles: toggles_label(toggles),
        stats: stats::summarise(&samples),
        samples,
    }
}

/// `bench ladder`'s outcome: the artifact, plus where it was written.
#[derive(Clone, Debug)]
pub struct Outcome {
    pub artifact: Artifact,
    pub json: String,
}

/// The scan, then the ladder, then the toggles, then the artifact.
pub fn run(options: &Options) -> Result<Outcome, String> {
    let (bed, bed_source, scan) = match options.bed {
        Some(bed) => (bed, "given", Vec::new()),
        None => {
            let scan = scan_goals(options)?;
            let bed = derive_bed(&scan, options.scan_pieces, options.scan_cap)?;
            (bed, "derived", scan)
        }
    };

    let games = Games::new(bed, options.seeds)?;
    if games.len() == 0 {
        return Err("the Split is empty".to_string());
    }
    let bed_fingerprint = games.fingerprint();
    let bed_name = bed.name();

    // Rungs 1–3: the Evaluators, at the shipped search decisions.
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut rows: Vec<RungRow> = Vec::new();
    for id in EVALUATORS {
        let (table, table_name, features, published, architecture) = Table::for_id(id)?;
        let row = rung(
            &games,
            bed,
            options.width,
            id,
            table,
            table_name,
            features,
            Toggles::SHIPPED,
        );
        candidates.push(Candidate {
            id: row.id,
            table: row.table,
            features,
            published,
            architecture,
            samples: row.samples.clone(),
        });
        rows.push(row);
    }
    let verdict = cascade(&candidates)?;
    let winner = candidates
        .iter()
        .position(|candidate| candidate.id == verdict.winner)
        .ok_or_else(|| format!("the cascade's winner {} has no row", verdict.winner))?;

    // Rungs 4–6: one decision off the winner, one at a time.
    let mut toggle_pairs: Vec<Pair> = Vec::new();
    let mut toggle_verdicts: Vec<ToggleVerdict> = Vec::new();
    for id in ROWS[3..].iter().copied() {
        let feature = toggle_for_id(id)?;
        let row = rung(
            &games,
            bed,
            options.width,
            id,
            Table::for_id(verdict.winner)?.0,
            candidates[winner].table,
            candidates[winner].features,
            Toggles::SHIPPED.without(feature),
        );
        let judged = Candidate {
            id: row.id,
            table: row.table,
            features: row.features,
            published: false,
            architecture: candidates[winner].architecture,
            samples: row.samples.clone(),
        };
        let pair = compare(
            &judged,
            &candidates[winner],
            bootstrap_seed(&bed_name, options.width, id, &bed_fingerprint),
        );
        toggle_verdicts.push(toggle_verdict(
            pair.moved_seeds > 0,
            pair.moved_seeds,
            pair.discordant,
            pair.ci95,
        ));
        toggle_pairs.push(pair);
        rows.push(row);
    }

    // The published table's "vs winner" column: the Evaluators against the winner.
    let mut pairs: Vec<Pair> = Vec::new();
    for row in rows.iter().filter(|row| EVALUATORS.contains(&row.id)) {
        if row.id == verdict.winner {
            continue;
        }
        let pair_id = format!("{}-vs-{}", row.id, verdict.winner);
        let judged = Candidate {
            id: row.id,
            table: row.table,
            features: row.features,
            published: row.id == "e1",
            architecture: candidates[winner].architecture,
            samples: row.samples.clone(),
        };
        pairs.push(compare(
            &judged,
            &candidates[winner],
            bootstrap_seed(&bed_name, options.width, &pair_id, &bed_fingerprint),
        ));
    }

    let artifact = Artifact {
        date: options.date.clone(),
        engine_commit: options.engine_commit.clone(),
        machine: options.machine.clone(),
        bed,
        bed_source: bed_source.to_string(),
        scan,
        scan_pieces: options.scan_pieces,
        scan_cap: options.scan_cap,
        split: options.seeds,
        width: options.width,
        width_cap: options.width_cap,
        quality_fingerprint: options.quality_fingerprint.clone(),
        bed_fingerprint,
        rows,
        pairs,
        verdict,
        toggles: toggle_verdicts,
        toggle_pairs,
        saturation: options.saturation.clone(),
    };
    let json = render_json(&artifact);
    write(&options.out, &json)?;
    write(&options.rows_path, &render_rows(&artifact))?;
    write(&options.docs_path, &render_markdown(&artifact))?;
    Ok(Outcome { artifact, json })
}

/// The goal scan: E0 only, because E0 is the weakest Evaluator and the rule is
/// written as "until the weakest one drops below 99 %".
fn scan_goals(options: &Options) -> Result<Vec<ScanRow>, String> {
    let games = Games::new(
        Bed {
            goal: SCAN_FIRST_GOAL,
            pieces: options.scan_pieces,
            cap: options.scan_cap,
        },
        options.seeds,
    )?;
    let mut rows: Vec<ScanRow> = Vec::new();
    let mut goal = SCAN_FIRST_GOAL;
    while goal <= engine::search::MAX_GOAL {
        let samples = measure(
            &games,
            Bed {
                goal,
                pieces: options.scan_pieces,
                cap: options.scan_cap,
            },
            options.width,
            Table::E0,
            Toggles::SHIPPED,
        );
        let summary = stats::summarise(&samples);
        rows.push(ScanRow {
            goal,
            wins: summary.wins,
            games: summary.games,
            mean_lines: summary.mean_lines,
            evals_mean: summary.evals_mean,
        });
        if scan_is_done(&rows) {
            break;
        }
        goal += 1;
    }
    Ok(rows)
}

// ------------------------------------------------------------------ the writing

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    std::fs::write(path, text).map_err(|error| format!("write {}: {error}", path.display()))
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// `1000..=1023` as `1,000–1,023`, for the labels a human reads.
fn thousands(value: u32) -> String {
    let text = value.to_string();
    let mut out = String::new();
    for (index, digit) in text.chars().enumerate() {
        if index > 0 && (text.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

fn render_json(artifact: &Artifact) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str(&format!("  \"format\": {VERSION},\n"));
    out.push_str("  \"kind\": \"ablation-ladder\",\n");
    out.push_str(&format!(
        "  \"generated_at\": \"{}\",\n",
        escape(&artifact.date)
    ));
    out.push_str("  \"engine\": {\n");
    out.push_str(&format!(
        "    \"commit\": \"{}\",\n",
        escape(&artifact.engine_commit)
    ));
    out.push_str("    \"profile\": \"release: lto=fat, codegen-units=1, panic=abort, strip=true\"\n");
    out.push_str("  },\n");
    out.push_str(&format!(
        "  \"machine\": {{ \"label\": \"{}\" }},\n",
        escape(&artifact.machine)
    ));

    // §9.1, quoted from the artifact that measured it.
    out.push_str("  \"saturation\": {\n");
    out.push_str(&format!(
        "    \"checked_at_width\": {},\n",
        artifact.saturation.checked_at_width
    ));
    out.push_str(&format!(
        "    \"saturated\": {},\n",
        !artifact.saturation.beds.is_empty()
    ));
    out.push_str("    \"beds\": {");
    out.push_str(
        &artifact
            .saturation
            .beds
            .iter()
            .map(|(name, wins, games)| {
                format!(
                    "\"{}\": {{ \"wins\": {wins}, \"games\": {games} }}",
                    escape(name)
                )
            })
            .collect::<Vec<_>>()
            .join(", "),
    );
    out.push_str("},\n");
    out.push_str(&format!(
        "    \"source\": \"{}\"\n",
        escape(&artifact.saturation.source)
    ));
    out.push_str("  },\n");

    // The bed, its derivation and the split it was measured on.
    out.push_str(&format!(
        "  \"bed\": {{ \"name\": \"{}\", \"goal\": {}, \"pieces\": {}, \"cap\": {}, \
         \"committed_dataset\": {}, \"fingerprint\": \"{}\", \"generator_version\": 1 }},\n",
        artifact.bed.name(),
        artifact.bed.goal,
        artifact.bed.pieces,
        artifact.bed.cap,
        artifact.bed.is_shipped(),
        artifact.bed_fingerprint
    ));
    out.push_str(&format!(
        "  \"bed_source\": \"{}\",\n",
        escape(&artifact.bed_source)
    ));
    out.push_str("  \"derivation\": {\n");
    out.push_str(
        "    \"rule\": \"the first goal, scanning upward from 16 at 160 pieces and cap 7 on the \
         tuning Split at DEFAULT_BEAM, at which the weakest evaluator (E0) wins fewer than 99 % of \
         games; the bed is derived rather than pre-registered\",\n",
    );
    out.push_str(&format!("    \"scan_pieces\": {},\n", artifact.scan_pieces));
    out.push_str(&format!("    \"scan_cap\": {},\n", artifact.scan_cap));
    out.push_str(&format!(
        "    \"line_ceiling\": {},\n",
        line_ceiling(artifact.scan_pieces)
    ));
    out.push_str(&format!(
        "    \"first_goal_below_99\": {},\n",
        first_goal_below_saturation(&artifact.scan)
            .map(|index| artifact.scan[index].goal.to_string())
            .unwrap_or_else(|| "none".to_string())
    ));
    out.push_str(&format!(
        "    \"unreachable_from\": {},\n",
        unreachable_from(&artifact.scan)
            .map(|goal| goal.to_string())
            .unwrap_or_else(|| "none".to_string())
    ));
    out.push_str("    \"scan\": [\n");
    for (index, row) in artifact.scan.iter().enumerate() {
        out.push_str(&format!(
            "      {{ \"goal\": {}, \"wins\": {}, \"games\": {}, \"mean_lines\": {:.4}, \
             \"evals_mean\": {:.1}, \"saturated\": {}, \"below_99\": {} }}{}\n",
            row.goal,
            row.wins,
            row.games,
            row.mean_lines,
            row.evals_mean,
            row.saturated(),
            !row.saturated(),
            if index + 1 == artifact.scan.len() {
                ""
            } else {
                ","
            }
        ));
    }
    out.push_str("    ]\n");
    out.push_str("  },\n");
    out.push_str(&format!(
        "  \"split\": {{ \"first\": {}, \"last\": {} }},\n",
        artifact.split.0, artifact.split.1
    ));
    out.push_str(&format!(
        "  \"width\": {{ \"default_beam\": {}, \"width_cap\": {}, \"quality_fingerprint\": \
         \"{}\" }},\n",
        artifact.width,
        artifact.width_cap,
        escape(&artifact.quality_fingerprint)
    ));
    out.push_str(&format!(
        "  \"resamples\": {BOOTSTRAP_RESAMPLES},\n  \"alpha_inverse\": {},\n",
        width::ALPHA_INVERSE
    ));

    // The rungs.
    out.push_str("  \"rows\": [\n");
    for (index, row) in artifact.rows.iter().enumerate() {
        let s = &row.stats;
        out.push_str(&format!(
            "    {{ \"id\": \"{}\", \"table\": \"{}\", \"features\": {}, \"toggles\": \"{}\", \
             \"wins\": {}, \"games\": {}, \"win_rate\": {:.6}, \"win_rate_ci95\": [{:.6}, {:.6}], \
             \"mean_lines\": {:.4}, \"mean_lines_on_wins\": {:.4}, \"evals_median\": {:.1}, \
             \"evals_p90\": {:.1}, \"evals_mean\": {:.1}, \"ms_median\": {:.4}, \"ms_p90\": {:.4} \
             }}{}\n",
            escape(row.id),
            escape(row.table),
            row.features,
            escape(&row.toggles),
            s.wins,
            s.games,
            s.win_rate,
            s.win_rate_ci95[0],
            s.win_rate_ci95[1],
            s.mean_lines,
            s.mean_lines_on_wins,
            s.evals_median,
            s.evals_p90,
            s.evals_mean,
            s.ms_median,
            s.ms_p90,
            if index + 1 == artifact.rows.len() {
                ""
            } else {
                ","
            }
        ));
    }
    out.push_str("  ],\n");

    // The pairs, with the 2×2 table each p-value came from.
    out.push_str("  \"pairs\": [\n");
    let all: Vec<&Pair> = artifact
        .pairs
        .iter()
        .chain(artifact.toggle_pairs.iter())
        .collect();
    for (index, pair) in all.iter().enumerate() {
        out.push_str(&format!(
            "    {{ \"judged\": \"{}\", \"reference\": \"{}\", \"both\": {}, \"only_judged\": {}, \
             \"only_reference\": {}, \"neither\": {}, \"games\": {}, \"n_disc\": {}, \
             \"rejects_1_100\": {}, \"delta_mean_lines\": {:.4}, \"ci95\": [{:.4}, {:.4}], \
             \"ci95_excludes_zero\": {}, \"boot_seed\": \"0x{:016x}\", \"resamples\": \
             {BOOTSTRAP_RESAMPLES}, \"evals_median\": [{:.1}, {:.1}], \"evals_differ_seeds\": {}, \
             \"moved_seeds\": {} }}{}\n",
            escape(pair.judged),
            escape(pair.reference),
            pair.both,
            pair.only_judged,
            pair.only_reference,
            pair.neither,
            pair.games(),
            pair.discordant.n(),
            pair.rejects,
            pair.delta_mean_lines,
            pair.ci95[0],
            pair.ci95[1],
            pair.ci95_excludes_zero(),
            pair.boot_seed,
            pair.evals_median_judged,
            pair.evals_median_reference,
            pair.evals_differ_seeds,
            pair.moved_seeds,
            if index + 1 == all.len() { "" } else { "," }
        ));
    }
    out.push_str("  ],\n");

    // The cascade, as the procedure that ran.
    out.push_str("  \"cascade\": {\n");
    out.push_str(
        "    \"rule\": \"highest win rate, where a difference with exact McNemar p >= 0.01 is a \
         tie; a tie goes to fewer evaluations per solve, then to fewer features, then to the \
         published table (\\u00a79.4: E1 ships unless E2 clears p < 0.01)\",\n",
    );
    out.push_str(&format!(
        "    \"winner\": \"{}\",\n    \"leg\": \"{}\",\n    \"tied\": [{}],\n",
        escape(artifact.verdict.winner),
        artifact.verdict.leg.as_str(),
        artifact
            .verdict
            .tied
            .iter()
            .map(|id| format!("\"{id}\""))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    out.push_str("    \"steps\": [\n");
    for (index, step) in artifact.verdict.steps.iter().enumerate() {
        out.push_str(&format!(
            "      \"{}\"{}\n",
            escape(step),
            if index + 1 == artifact.verdict.steps.len() {
                ""
            } else {
                ","
            }
        ));
    }
    out.push_str("    ]\n  },\n");

    // The toggles.
    out.push_str("  \"toggles\": [\n");
    for (index, pair) in artifact.toggle_pairs.iter().enumerate() {
        out.push_str(&format!(
            "    {{ \"id\": \"{}\", \"off\": \"{}\", \"verdict\": \"{}\", \"moved_seeds\": {}, \
             \"n_disc\": {}, \"rejects_1_100\": {}, \"ci95\": [{:.4}, {:.4}], \
             \"ci95_excludes_zero\": {} }}{}\n",
            escape(pair.judged),
            escape(pair.judged.trim_end_matches("-off")),
            artifact.toggles[index].as_str(),
            pair.moved_seeds,
            pair.discordant.n(),
            pair.rejects,
            pair.ci95[0],
            pair.ci95[1],
            pair.ci95_excludes_zero(),
            if index + 1 == artifact.toggle_pairs.len() {
                ""
            } else {
                ","
            }
        ));
    }
    out.push_str("  ],\n");

    // The one statement ticket 09 and ticket 16 read.
    let shipped = artifact.row(artifact.verdict.winner);
    out.push_str("  \"shipped\": {\n");
    out.push_str(&format!(
        "    \"evaluator\": \"{}\",\n    \"table\": \"{}\",\n    \"features\": {},\n    \
         \"toggles\": \"{}\",\n    \"deleted_toggles\": [{}],\n",
        escape(shipped.id),
        escape(shipped.table),
        shipped.features,
        escape(&shipped.toggles),
        artifact
            .deleted_toggles()
            .iter()
            .map(|id| format!("\"{id}\""))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    out.push_str(&format!(
        "    \"decided_by\": \"\\u00a79.4 cascade on the bed above, leg: {}\",\n    \
         \"read_by\": \"ticket 09 (Reference run) and ticket 16 (Explainer) read this block; \
         docs/measurements/ablation-ladder.md prints the same verdict\"\n",
        escape(artifact.verdict.leg.as_str())
    ));
    out.push_str("  },\n");

    // The labels the published table must carry.
    out.push_str("  \"labels\": {\n");
    out.push_str(&format!(
        "    \"bed\": \"{}\",\n",
        escape(&bed_label(artifact))
    ));
    out.push_str(&format!(
        "    \"split\": \"{}\",\n",
        escape(&split_label(artifact))
    ));
    out.push_str(&format!(
        "    \"width\": \"DEFAULT_BEAM = {} (from data/measurements/width-curve.json, quality \
         fingerprint {}), width cap {}\",\n",
        artifact.width,
        escape(&artifact.quality_fingerprint),
        artifact.width_cap
    ));
    out.push_str(&format!(
        "    \"machine\": \"{}\",\n",
        escape(&artifact.machine)
    ));
    out.push_str(&format!(
        "    \"dataset\": \"generated from engine::generate_game (generator_version 1), bed \
         fingerprint {}, engine commit {}, {}\",\n",
        artifact.bed_fingerprint,
        escape(&artifact.engine_commit),
        escape(&artifact.date)
    ));
    out.push_str(
        "    \"rules\": \"game over = spawn-blocked; no-suicide fallback on (\\u00a75.3); goal \
         reached => search stops; lines on failure are the beam's high-water mark\"\n",
    );
    out.push_str("  }\n");
    out.push_str("}\n");
    out
}

fn bed_label(artifact: &Artifact) -> String {
    format!(
        "{} — goal {}, {} pieces, cap {} ({})",
        artifact.bed.name(),
        artifact.bed.goal,
        artifact.bed.pieces,
        artifact.bed.cap,
        if artifact.bed_source == "derived" {
            "derived, not pre-registered: the first goal at 160 pieces and cap 7 at which E0 wins \
             under 99 % (notes/08 run decision); never shipped, never in a Run"
        } else {
            "handed to the ladder"
        }
    )
}

fn split_label(artifact: &Artifact) -> String {
    format!(
        "{}–{} (the tuning Split that §6 reserves for weight tuning, ablations and width curves)",
        thousands(artifact.split.0),
        thousands(artifact.split.1)
    )
}

fn render_rows(artifact: &Artifact) -> String {
    let mut out = String::new();
    for row in &artifact.rows {
        for sample in &row.samples {
            out.push_str(&format!(
                "{{\"row\":\"{}\",\"table\":\"{}\",\"bed\":\"{}\",\"width\":{},\"goal\":{},\
                 \"split\":\"{}-{}\",\"seed\":{},\"won\":{},\"lines\":{},\"evals\":{},\
                 \"moves\":{},\"ms\":{:.4},\"bed_fingerprint\":\"{}\"}}\n",
                escape(row.id),
                escape(row.table),
                artifact.bed.name(),
                artifact.width,
                artifact.bed.goal,
                artifact.split.0,
                artifact.split.1,
                sample.seed,
                sample.won,
                sample.lines,
                sample.evals,
                sample.moves,
                sample.ms,
                artifact.bed_fingerprint
            ));
        }
    }
    out
}

/// The published comparison table: the one a human reads, with the labels ticket 08
/// requires — the bed, the Split and the machine.
pub fn render_markdown(artifact: &Artifact) -> String {
    let games = artifact.rows.first().map(|row| row.stats.games).unwrap_or(0);
    let mut out = String::new();
    out.push_str("# The ablation ladder: which Evaluator ships\n\n");
    out.push_str(
        "Generated by `bench ladder` (ticket 08) from `data/measurements/ablation-ladder.json`.\n\
         Every number here is measured, and the per-seed rows behind every number are in\n\
         `data/measurements/ablation-ladder-rows.jsonl`.\n\n",
    );
    out.push_str("| label | value |\n|---|---|\n");
    out.push_str(&format!("| Bed | {} |\n", bed_label(artifact)));
    out.push_str(&format!("| Split | {} |\n", split_label(artifact)));
    out.push_str(&format!(
        "| Width | `DEFAULT_BEAM` = {} (from `data/measurements/width-curve.json`, quality \
         fingerprint `{}`), width cap {} |\n",
        artifact.width, artifact.quality_fingerprint, artifact.width_cap
    ));
    out.push_str(&format!("| Machine | {} |\n", artifact.machine));
    out.push_str(&format!(
        "| Dataset | generated from `engine::generate_game` (generator_version 1), bed fingerprint \
         `{}`, engine commit `{}`, {} |\n",
        artifact.bed_fingerprint, artifact.engine_commit, artifact.date
    ));
    out.push_str(
        "| Rules | game over = spawn-blocked; no-suicide fallback on (§5.3); goal reached ⇒ search \
         stops |\n",
    );
    out.push_str(&format!(
        "| Statistics | paired per seed; exact McNemar (two-sided sign test, α = 1/100); paired \
         bootstrap 95 % CI over {} resamples with a pinned stream |\n\n",
        BOOTSTRAP_RESAMPLES
    ));

    if !artifact.scan.is_empty() {
        out.push_str("## The bed was derived, not pre-registered\n\n");
        out.push_str(
            "§9.1's saturation check fires on both shipped beds and on the validation-only bed, so \
             no bed\nthe design spec pre-registers can rank an evaluator. The operator's run \
             decision (notes/08)\ntherefore scans goals upward at 160 pieces and cap 7 on the \
             tuning Split and names the first\ngoal at which the weakest Evaluator (E0) wins fewer \
             than 99 % of games. This is that scan: every\ngoal tried, with E0's win count.\n\n",
        );
        out.push_str(
            "| goal | E0 wins | below 99 % | mean lines | mean evaluations |\n\
             |---|---|---|---|---|\n",
        );
        for row in &artifact.scan {
            out.push_str(&format!(
                "| {} | {}/{} | {} | {:.2} | {} |\n",
                row.goal,
                row.wins,
                row.games,
                if row.saturated() { "no" } else { "**yes**" },
                row.mean_lines,
                row.evals_mean as u64
            ));
        }
        let at_bed = artifact
            .scan
            .iter()
            .find(|row| row.goal == artifact.bed.goal)
            .map(|row| row.mean_lines)
            .unwrap_or(0.0);
        out.push_str(&format!(
            "\nThe {} line ceiling is what {} pieces can clear (4 cells each, 10 cells a row). Goal \
             {}\nsits above it because §5.7's pre-fill puts a stack on the Board before the first \
             piece, and E0's\nmean {:.2} Lines at goal {} is the measurement that it is reached \
             rather than assumed.\nGoals from {} were reached by nobody.\n\n",
            line_ceiling(artifact.scan_pieces),
            artifact.scan_pieces,
            artifact.bed.goal,
            at_bed,
            artifact.bed.goal,
            unreachable_from(&artifact.scan)
                .map(|goal| goal.to_string())
                .unwrap_or_else(|| "none".to_string())
        ));
    }

    out.push_str("## The table\n\n");
    out.push_str(
        "| row | table | features | toggles | wins | vs winner (b, c, n) | p < 0.01 | mean lines Δ \
         [95 % CI] | evals p50 | evals p90 | ms p50 | verdict |\n\
         |---|---|---|---|---|---|---|---|---|---|---|---|\n",
    );
    for row in &artifact.rows {
        let s = &row.stats;
        let pair = artifact.pair(row.id);
        let verdict = if row.id == artifact.verdict.winner {
            "**ships**".to_string()
        } else if let Some(index) = artifact
            .toggle_pairs
            .iter()
            .position(|pair| pair.judged == row.id)
        {
            match artifact.toggles[index] {
                ToggleVerdict::Kept => "kept (moves its axis)".to_string(),
                ToggleVerdict::Deleted => "**deleted from the engine**".to_string(),
            }
        } else {
            "not shipped".to_string()
        };
        let (bc, significant, delta) = match pair {
            Some(pair) => (
                format!(
                    "{}, {}, {}",
                    pair.only_judged,
                    pair.only_reference,
                    pair.discordant.n()
                ),
                if pair.rejects { "yes" } else { "no (tie)" },
                format!(
                    "{:+.4} [{:+.4}, {:+.4}]",
                    pair.delta_mean_lines, pair.ci95[0], pair.ci95[1]
                ),
            ),
            None => ("—".to_string(), "—", "—".to_string()),
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {}/{} | {} | {} | {} | {} | {} | {:.2} | {} |\n",
            row.id,
            row.table,
            row.features,
            row.toggles,
            s.wins,
            s.games,
            bc,
            significant,
            delta,
            s.evals_median as u64,
            s.evals_p90 as u64,
            s.ms_median,
            verdict
        ));
    }

    out.push_str("\n## The cascade\n\n");
    out.push_str(
        "§9.4: highest win rate; a difference with exact McNemar `p >= 0.01` is a tie; a tie goes to \
         fewer\nEvaluations per Solve, then to fewer Features. The legs actually run:\n\n",
    );
    for step in &artifact.verdict.steps {
        out.push_str(&format!("- {step}\n"));
    }
    out.push_str(&format!(
        "\n**Winner: `{}` ({}), decided by {}.** Tied at the start of the legs: {}.\n\n",
        artifact.verdict.winner,
        artifact.row(artifact.verdict.winner).table,
        artifact.verdict.leg.as_str(),
        artifact
            .verdict
            .tied
            .iter()
            .map(|id| format!("`{id}`"))
            .collect::<Vec<_>>()
            .join(", ")
    ));

    out.push_str("## The toggles\n\n");
    out.push_str(
        "One at a time off the winner. A toggle that moves nothing beyond its confidence interval is \
         deleted\nfrom the engine, not kept (§9.4). `moved seeds` is how many of the Split's games \
         changed any of\n`(won, lines, evals)` at all: a mechanism that never engaged cannot be \
         told from one that achieved\nnothing without it.\n\n",
    );
    out.push_str(
        "| toggle | moved seeds | flipped (b, c) | mean lines Δ [95 % CI] | verdict |\n\
         |---|---|---|---|---|\n",
    );
    for (index, pair) in artifact.toggle_pairs.iter().enumerate() {
        out.push_str(&format!(
            "| {} | {}/{} | {}, {} | {:+.4} [{:+.4}, {:+.4}] | {} |\n",
            pair.judged,
            pair.moved_seeds,
            games,
            pair.only_judged,
            pair.only_reference,
            pair.delta_mean_lines,
            pair.ci95[0],
            pair.ci95[1],
            artifact.toggles[index].as_str()
        ));
    }
    out.push('\n');
    out
}
