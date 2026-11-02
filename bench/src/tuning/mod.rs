//! Ticket 07: cross-entropy tuned Dellacherie–Thiery weights (E2), on the tuning
//! Split only.
//!
//! # What "cross-entropy" means here
//!
//! The optimisation heuristic, not a classification loss. Thiery & Scherrer,
//! *Improvements on Learning Tetris with Cross-Entropy* (ICGA J. 32, 2009) §3.2:
//! the function optimised is "the expected mean score achieved by the
//! corresponding controller". So the trainer samples **weight vectors**, plays
//! games, and ranks by **what the game did** — it never computes a likelihood of
//! the search's own placements. A placement-log-likelihood objective would be
//! self-distillation under a softmax that is invariant to positive rescaling of
//! the weights, and could not identify them at all.
//!
//! The pre-registered protocol is the paper's, with two additions that a finite
//! game needs:
//!
//! | knob | value | basis |
//! |---|---|---|
//! | μ₀, σ²₀ | 0, 100 | paper §4 |
//! | candidates `N` | 100 | paper §4 |
//! | elite fraction ρ | 0.10 | paper §4 |
//! | noise `Z` | 4, constant | paper §4 |
//! | iterations `K` | 40, early stop | paper Fig. 4 |
//! | games per candidate `G` | 8, common random numbers | our games are finite |
//! | variance smoothing α | 1.0 then 0.7 after 10 | keeps the elites' variance from collapsing |
//! | selection | best on a held-out 200-seed tuning slice | without it, best-of-4,000 overfits the 8-game minibatches |
//!
//! # The split guard
//!
//! Every game this module can reach comes from a [`TrainingDataset`], which is
//! constructible only from a [`TrainingSplit`], which has one inhabitant. There
//! is no `dataset::parse` call, no `games.txt` path and no `TEST_SPLIT` name
//! anywhere under `bench/src/tuning/`, and `bench/tests/tuning_split.rs` fails if
//! one appears. That is the ticket's "enforced in code and asserted by a test,
//! not left to discipline".
//!
//! # Where the randomness lives
//!
//! In [`rng`], in this crate. §5.3 forbids an RNG in `engine/` — visit order,
//! tie-breaking and dedup domination are observable contract — and nothing under
//! `engine/` imports it. A run is a pure function of `(tune_seed, params, bed,
//! width, dataset)`, which is why `bench tune --replay` can re-derive a committed
//! table's fingerprint from its committed seed.

pub mod q32;
pub mod record;
pub mod rng;
pub mod stats;

use std::path::{Path, PathBuf};

use engine::{Board, DellacherieThiery, DtWeights, Evaluator, PlacementCtx};

use crate::dataset::{TrainingDataset, TrainingSplit};
use crate::fingerprint::Fnv;
use crate::width::Bed;
use q32::{TrainDt, WEIGHT_BOUND};

/// FNV-1a-64 over the eight `i32` as little-endian bytes: a table's identity.
/// Re-exported here so the record, the CLI and the audit test all name one
/// function.
pub use stats::table_fnv;
use rng::SplitMix64;
use stats::{Sample, Stats};

/// The eight Features, in the order every vector in this module is indexed by.
/// Named once, from the engine, so a second ordering cannot appear.
pub const FEATURES: [&str; 8] = DtWeights::FEATURES;

/// The paper's starting point: a gaussian centred at zero with variance 100 per
/// coordinate.
pub const MU_0: f64 = 0.0;
pub const SIGMA_SQ_0: f64 = 100.0;

/// §5.6/§5.7's habit, one stream per purpose: the CE pool is 1,000–1,799 and the
/// held-out selection slice is 1,800–1,999. Both are inside the tuning Split.
pub const CE_POOL: (u32, u32) = (1_000, 1_799);
pub const VALIDATION: (u32, u32) = (1_800, 1_999);

/// The pre-registered rejection rule, quoted into every run so it cannot be
/// re-read after the fact.
pub const RULE: &str = "notes/07 §6.1, pre-registered before the run: (1) win rate -- E1 is worse than published if its win rate is higher and the exact two-sided McNemar test on the discordant pairs rejects at p < 0.01; (2) mean lines -- only when the win rates tie, E2 is worse if the paired bootstrap 95% CI of lines_E2 - lines_E1 lies entirely below 0; (3) evaluations per solve is §9.4's tie-break, never a rejection. Saturation is recorded and not silently accepted: win_rate_axis is \"no-resolution\" when both tables win >= 99% of the tuning Split, so a tie there is a statement about the bed, not about the tables.";

// ------------------------------------------------------------------ the knobs

/// The cross-entropy schedule. Every field is recorded in the run, because a
/// hyper-parameter that is not in the record is a hyper-parameter nobody can
/// reproduce.
#[derive(Clone, Debug, PartialEq)]
pub struct CeParams {
    pub candidates: u32,
    pub games_per_candidate: u32,
    pub iterations: u32,
    /// `⌊ρ·N⌋`, floored at 1.
    pub elite_fraction: f64,
    /// `Z`: added to the variance every iteration, constant (paper §4).
    pub noise: f64,
    pub smoothing_early: f64,
    pub smoothing_late: f64,
    pub smoothing_switch: u32,
    /// `σ²_min`.
    pub variance_floor: f64,
    /// `|w| ≤ 1024` (see [`WEIGHT_BOUND`]).
    pub weight_bound: f64,
    pub mu0: f64,
    pub sigma_sq0: f64,
    /// Stop after this many iterations with no improvement of the best held-out
    /// fitness.
    pub max_stale: u32,
    /// The minibatch pool, inside the tuning Split.
    pub ce_pool: (u32, u32),
    /// The held-out selection slice, inside the tuning Split.
    pub validation: (u32, u32),
    pub bootstrap_resamples: u32,
}

impl CeParams {
    /// The default run: the paper's protocol at `G = 8` with a held-out slice.
    pub fn paper() -> CeParams {
        CeParams {
            candidates: 100,
            games_per_candidate: 8,
            iterations: 40,
            elite_fraction: 0.10,
            noise: 4.0,
            smoothing_early: 1.0,
            smoothing_late: 0.7,
            smoothing_switch: 10,
            variance_floor: 0.01,
            weight_bound: WEIGHT_BOUND,
            mu0: MU_0,
            sigma_sq0: SIGMA_SQ_0,
            max_stale: 5,
            ce_pool: CE_POOL,
            validation: VALIDATION,
            bootstrap_resamples: 10_000,
        }
    }

    /// A four-candidate, two-game, two-iteration run on a four-seed pool and a
    /// four-seed slice: the reproducibility proof that fits in a test.
    pub fn miniature() -> CeParams {
        CeParams {
            candidates: 4,
            games_per_candidate: 2,
            iterations: 2,
            elite_fraction: 0.25,
            noise: 4.0,
            smoothing_early: 1.0,
            smoothing_late: 0.7,
            smoothing_switch: 1,
            variance_floor: 0.01,
            weight_bound: WEIGHT_BOUND,
            mu0: MU_0,
            sigma_sq0: SIGMA_SQ_0,
            max_stale: 5,
            ce_pool: (1_000, 1_003),
            validation: (1_900, 1_903),
            bootstrap_resamples: 200,
        }
    }

    pub fn ce_pool_len(&self) -> u32 {
        self.ce_pool.1 - self.ce_pool.0 + 1
    }

    pub fn elite_count(&self) -> usize {
        let count = (self.elite_fraction * self.candidates as f64).floor() as usize;
        count.max(1).min(self.candidates as usize)
    }

    /// The record's `params` block.
    pub fn render(&self) -> Vec<(String, String)> {
        vec![
            ("candidates".to_string(), self.candidates.to_string()),
            (
                "games_per_candidate".to_string(),
                self.games_per_candidate.to_string(),
            ),
            ("iterations".to_string(), self.iterations.to_string()),
            ("elite_count".to_string(), self.elite_count().to_string()),
            (
                "elite_fraction".to_string(),
                record::number_for(self.elite_fraction),
            ),
            ("noise".to_string(), record::number_for(self.noise)),
            (
                "variance_smoothing".to_string(),
                format!(
                    "[{}, {}]",
                    record::number_for(self.smoothing_early),
                    record::number_for(self.smoothing_late)
                ),
            ),
            (
                "variance_smoothing_switch".to_string(),
                self.smoothing_switch.to_string(),
            ),
            (
                "variance_floor".to_string(),
                record::number_for(self.variance_floor),
            ),
            (
                "weight_bound".to_string(),
                record::number_for(self.weight_bound),
            ),
            ("mu0".to_string(), record::number_for(self.mu0)),
            (
                "sigma_sq0".to_string(),
                record::number_for(self.sigma_sq0),
            ),
            ("max_stale".to_string(), self.max_stale.to_string()),
            (
                "ce_pool".to_string(),
                format!("\"{}-{}\"", self.ce_pool.0, self.ce_pool.1),
            ),
            (
                "validation_seeds".to_string(),
                format!("\"{}-{}\"", self.validation.0, self.validation.1),
            ),
            (
                "bootstrap_resamples".to_string(),
                self.bootstrap_resamples.to_string(),
            ),
            (
                "fitness".to_string(),
                "\"1000*won_rate + mean_lines + 0.01*mean_pieces_saved\"".to_string(),
            ),
            (
                "transport".to_string(),
                "\"Q32 i64 per candidate, >>32; committed as Q16 i32 (round half away from zero)\""
                    .to_string(),
            ),
        ]
    }
}

// ---------------------------------------------------------------- a table swap

/// One of the four weight tables a run measures, as a value the worker threads
/// can copy. E0 and the three Dellacherie–Thiery tables differ in their data,
/// never in their code — which is the ticket's "E2 is a configuration swap on E1,
/// not a third Evaluator", made mechanical.
#[derive(Clone, Copy, Debug)]
pub enum Table {
    Heuristic(engine::HeuristicWeights),
    Dt(DtWeights),
    /// A CE candidate, at Q32 transport precision.
    Training([f64; 8]),
}

impl Table {
    /// Run `body` with the engine's own evaluator for this table. The evaluator is
    /// constructed per call (eight multiplies) so the closure stays `Sync` and the
    /// scoring path stays the engine's.
    fn with<R>(&self, body: impl FnOnce(&dyn Evaluator) -> R) -> R {
        match self {
            Table::Heuristic(weights) => body(&engine::Heuristic(*weights)),
            Table::Dt(weights) => body(&DellacherieThiery(*weights)),
            Table::Training(decimal) => body(&TrainDt::from_decimal(decimal)),
        }
    }
}

/// One Solve's outcome, as the trainer counts it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Outcome {
    pub won: bool,
    pub lines: u32,
    pub evals: u32,
    pub moves: u32,
    pub ms: f64,
}

// --------------------------------------------------------------- parallel grid

/// `(table, game)` pairs solved across the machine's cores, **in index order**.
///
/// Each pair is independent — the search has no shared state and the engine has
/// no RNG — so the split is deterministic even though the scheduling is not: the
/// results are collected by index and every reduction afterwards runs on the main
/// thread in that order. Notes/07 §7.5's "no parallel reduction" is therefore
/// satisfied by construction rather than by care.
fn solve_grid(tables: &[Table], games: &[&engine::Game], bed: Bed, width: u32) -> Vec<Outcome> {
    let per_table = games.len();
    let total = tables.len() * per_table;
    let work = |task: usize| -> Outcome {
        let table = tables[task / per_table];
        let game = games[task % per_table];
        let started = std::time::Instant::now();
        let result = table.with(|evaluator| {
            engine::solve(
                &game.board,
                game.sequence.as_slice(),
                &engine::SolveConfig::new(bed.goal, width),
                evaluator,
            )
        });
        Outcome {
            won: result.won,
            lines: result.lines,
            evals: result.evaluations,
            moves: result.moves.len() as u32,
            ms: started.elapsed().as_secs_f64() * 1000.0,
        }
    };
    map_parallel(total, &work)
}

/// `f(0..n)` across the machine's cores, returned in index order.
fn map_parallel(n: usize, f: &(impl Fn(usize) -> Outcome + Sync)) -> Vec<Outcome> {
    let threads = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .min(n.max(1));
    if threads <= 1 || n < 16 {
        return (0..n).map(f).collect();
    }
    let chunk = n.div_ceil(threads);
    let mut parts: Vec<Vec<Outcome>> = Vec::new();
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for start in (0..n).step_by(chunk) {
            let end = (start + chunk).min(n);
            handles.push(scope.spawn(move || (start..end).map(f).collect::<Vec<Outcome>>()));
        }
        for handle in handles {
            parts.push(handle.join().expect("a worker thread panicked"));
        }
    });
    parts.into_iter().flatten().collect()
}

/// The trainer's scalar, pre-registered in [`CeParams::render`]:
///
/// ```text
/// fitness(w) = 1000 · won_rate(w) + mean_lines(w) + 0.01 · mean_pieces_saved(w)
/// ```
///
/// `mean_lines` alone has no gradient among wins — on a win the Solve stops at the
/// Goal — so the third term is what keeps CE moving on a bed that saturates at
/// 100 %. `pieces_saved` rewards reaching the Goal earlier, which is the closest
/// outcome-side proxy for "stronger" and does not reward cheap trimming the way
/// `−evaluations` would.
fn fitness(outcomes: &[Outcome], pieces: u32) -> f64 {
    let n = outcomes.len() as f64;
    if outcomes.is_empty() {
        return 0.0;
    }
    let mut wins = 0u32;
    let mut lines = 0.0f64;
    let mut saved = 0.0f64;
    for outcome in outcomes {
        lines += outcome.lines as f64;
        if outcome.won {
            wins += 1;
            if pieces > outcome.moves {
                saved += (pieces - outcome.moves) as f64;
            }
        }
    }
    let mean_saved = if wins > 0 { saved / wins as f64 } else { 0.0 };
    1_000.0 * (wins as f64) / n + lines / n + 0.01 * mean_saved
}

// ------------------------------------------------------------------ the search

/// What one cross-entropy search produced. Everything a replay needs to check.
#[derive(Clone, Debug)]
pub struct SearchOutcome {
    pub weights_decimal: [f64; 8],
    pub weights_q16: [i32; 8],
    pub table_fnv: String,
    pub minibatch_fnv: String,
    pub iterations_run: u32,
    pub stop_reason: String,
    /// The game seeds each iteration's minibatch drew, in draw order.
    pub per_iteration_seeds: Vec<Vec<u32>>,
    /// Every tuning seed the search itself played (minibatches and the held-out
    /// slice), ascending.
    pub seeds_played: Vec<u32>,
    pub seed_list_fnv: String,
    /// The held-out fitness of the selected vector, so the record can say how
    /// good the choice looked on games it was not tuned on.
    pub selection_fitness: f64,
}

/// Cross-entropy over weight vectors, on the tuning Split, at a fixed width.
///
/// The draw order is fixed and documented, because reordering the draws is a
/// different run: per iteration, the `G` game seeds (index order, without
/// replacement), then for candidate `i = 0..N-1` and dimension `j = 0..7` one
/// normal draw.
pub fn search(
    dataset: &TrainingDataset,
    bed: &Bed,
    width: u32,
    params: &CeParams,
    tune_seed: u64,
) -> Result<SearchOutcome, String> {
    let games = dataset.games();
    let first_seed = games
        .first()
        .map(|game| game.seed)
        .ok_or_else(|| "the training dataset is empty".to_string())?;
    let index_of = |seed: u32| -> Result<usize, String> {
        let offset = seed.checked_sub(first_seed).ok_or_else(|| {
            format!("seed {seed} is below the dataset's first seed {first_seed}")
        })? as usize;
        if offset >= games.len() {
            return Err(format!("seed {seed} is past the dataset's last seed"));
        }
        Ok(offset)
    };
    if params.candidates == 0 || params.games_per_candidate == 0 || params.iterations == 0 {
        return Err("candidates, games_per_candidate and iterations must all be positive".into());
    }
    if params.games_per_candidate > params.ce_pool_len() {
        return Err(format!(
            "a minibatch of {} games cannot be drawn without replacement from a {}-seed pool",
            params.games_per_candidate,
            params.ce_pool_len()
        ));
    }

    let pool: Vec<&engine::Game> = {
        let mut out = Vec::with_capacity(params.ce_pool_len() as usize);
        for seed in params.ce_pool.0..=params.ce_pool.1 {
            out.push(&games[index_of(seed)?]);
        }
        out
    };
    let validation: Vec<&engine::Game> = {
        let mut out = Vec::with_capacity((params.validation.1 - params.validation.0 + 1) as usize);
        for seed in params.validation.0..=params.validation.1 {
            out.push(&games[index_of(seed)?]);
        }
        out
    };

    let mut rng = SplitMix64::tune_stream(tune_seed);
    let mut mu = [params.mu0; 8];
    let mut variance = [params.sigma_sq0; 8];
    let mut minibatch_fnv = Fnv::new();
    let mut per_iteration_seeds: Vec<Vec<u32>> = Vec::new();
    let mut played: Vec<u32> = Vec::new();

    let mut best: Option<([f64; 8], f64)> = None;
    let mut stale = 0u32;
    let mut iterations_run = 0u32;
    let mut stop_reason = "iterations_complete".to_string();

    for iteration in 0..params.iterations {
        // 1. the minibatch: the same games for every candidate (common random
        //    numbers), drawn without replacement, in draw order.
        let picks = rng.distinct_below(params.games_per_candidate, params.ce_pool_len());
        let mut seeds = Vec::with_capacity(picks.len());
        let mut batch: Vec<&engine::Game> = Vec::with_capacity(picks.len());
        for pick in picks {
            let seed = params.ce_pool.0 + pick;
            seeds.push(seed);
            minibatch_fnv.u32_le(seed);
            played.push(seed);
            batch.push(pool[pick as usize]);
        }
        per_iteration_seeds.push(seeds);

        // 2. the candidates.
        let mut candidates: Vec<[f64; 8]> = Vec::with_capacity(params.candidates as usize);
        for _ in 0..params.candidates {
            let mut weights = [0.0f64; 8];
            for (dimension, value) in weights.iter_mut().enumerate() {
                let sigma = variance[dimension].sqrt();
                let sample = mu[dimension] + sigma * rng.normal();
                *value = sample.clamp(-params.weight_bound, params.weight_bound);
            }
            candidates.push(weights);
        }

        // 3. one grid over (candidate, game): fully parallel, reduced in order.
        let tables: Vec<Table> = candidates.iter().map(|w| Table::Training(*w)).collect();
        let outcomes = solve_grid(&tables, &batch, *bed, width);
        let per_candidate = batch.len();
        let fitnesses: Vec<f64> = (0..candidates.len())
            .map(|index| {
                fitness(
                    &outcomes[index * per_candidate..(index + 1) * per_candidate],
                    bed.pieces,
                )
            })
            .collect();

        // 4. the elites: fitness descending, ties by candidate index, so an
        //    unstable sort or a hash order cannot reorder the mean.
        let mut order: Vec<usize> = (0..candidates.len()).collect();
        order.sort_by(|a, b| {
            fitnesses[*b]
                .partial_cmp(&fitnesses[*a])
                .expect("a fitness is finite")
                .then(a.cmp(b))
        });
        let elite_count = params.elite_count();
        let elites = &order[..elite_count];

        // 5. the update, in elite rank order.
        let mut mu_next = [0.0f64; 8];
        for elite in elites {
            for dimension in 0..8 {
                mu_next[dimension] += candidates[*elite][dimension];
            }
        }
        for value in mu_next.iter_mut() {
            *value /= elite_count as f64;
        }
        let mut variance_next = [0.0f64; 8];
        for elite in elites {
            for dimension in 0..8 {
                let delta = candidates[*elite][dimension] - mu_next[dimension];
                variance_next[dimension] += delta * delta;
            }
        }
        let alpha = if iteration < params.smoothing_switch {
            params.smoothing_early
        } else {
            params.smoothing_late
        };
        for dimension in 0..8 {
            variance_next[dimension] /= elite_count as f64;
            variance[dimension] = ((1.0 - alpha) * variance[dimension]
                + alpha * variance_next[dimension]
                + params.noise)
                .max(params.variance_floor);
        }
        mu = mu_next;

        // 6. the held-out slice decides which candidate is *kept*: best-of-4,000
        //    on 8-game minibatches would otherwise overfit them.
        let best_index = order[0];
        let held_out = solve_grid(&[Table::Training(candidates[best_index])], &validation, *bed, width);
        let held_out_fitness = fitness(&held_out, bed.pieces);
        for game in &validation {
            played.push(game.seed);
        }
        match &best {
            Some((_, previous)) if *previous >= held_out_fitness => stale += 1,
            _ => {
                best = Some((candidates[best_index], held_out_fitness));
                stale = 0;
            }
        }

        iterations_run = iteration + 1;

        // 7. the stops.
        if variance.iter().all(|value| value.sqrt() < params.variance_floor.sqrt()) {
            stop_reason = "variance_floor".to_string();
            break;
        }
        if stale >= params.max_stale {
            stop_reason = "no_improvement".to_string();
            break;
        }
    }

    let (weights_decimal, selection_fitness) = best.ok_or_else(|| {
        "the search produced no candidate: every iteration was skipped".to_string()
    })?;
    played.sort_unstable();
    played.dedup();
    let weights_q16 = q32::to_q16(&weights_decimal);
    Ok(SearchOutcome {
        weights_decimal,
        weights_q16,
        table_fnv: stats::table_fnv(&weights_q16),
        minibatch_fnv: crate::fingerprint::hex(minibatch_fnv.finish()),
        iterations_run,
        stop_reason,
        per_iteration_seeds,
        seeds_played: played,
        seed_list_fnv: dataset.seed_list_fnv(),
        selection_fitness,
    })
}

// --------------------------------------------------------------- the measurement

/// Measure one table on every game of the tuning Split, in seed order.
pub fn measure(dataset: &TrainingDataset, bed: &Bed, width: u32, table: Table) -> Vec<Sample> {
    let games: Vec<&engine::Game> = dataset.games().iter().collect();
    let outcomes = solve_grid(&[table], &games, *bed, width);
    games
        .iter()
        .zip(outcomes)
        .map(|(game, outcome)| Sample {
            seed: game.seed,
            won: outcome.won,
            lines: outcome.lines,
            evals: outcome.evals,
            moves: outcome.moves,
            ms: outcome.ms,
            pieces: bed.pieces,
        })
        .collect()
}

// ------------------------------------------------------------------- the run

/// The tables a run measures. E0 and the BCTS vector are the calibration points
/// §9.4's ladder needs; E1 is the published baseline the rejection rule compares
/// against; E2 is the candidate.
pub const EVALUATORS: [&str; 4] = ["e0", "e1", "bcts", "e2"];

/// Everything one run needs that is not derived from the search.
#[derive(Clone, Debug)]
pub struct RunInputs {
    pub run: u32,
    pub date: String,
    pub bed: Bed,
    pub bed_source: String,
    pub dataset_fingerprint: String,
    pub generator_version: u32,
    pub width: u32,
    pub width_curve: record::WidthCurveRef,
    pub tune_seed: u64,
    pub params: CeParams,
    pub machine: record::Machine,
}

/// Search, measure the four tables on all 1,000 tuning seeds, apply the
/// pre-registered rule, and build the record entry plus its rows.
pub fn train(
    dataset: &TrainingDataset,
    inputs: &RunInputs,
) -> Result<(record::Run, Vec<record::Row>), String> {
    let outcome = search(dataset, &inputs.bed, inputs.width, &inputs.params, inputs.tune_seed)?;

    let tables: [(&str, &str, Table); 4] = [
        ("e0", "E0_HEURISTIC", Table::Heuristic(engine::E0_HEURISTIC)),
        ("e1", "E1_PUBLISHED", Table::Dt(engine::E1_PUBLISHED)),
        ("bcts", "BCTS_PUBLISHED", Table::Dt(q32::bcts_table())),
        ("e2", "E2_TUNED", Table::Dt(DtWeights::from_array(outcome.weights_q16))),
    ];

    let mut rows: Vec<record::Row> = Vec::new();
    let mut samples: Vec<(&str, Vec<Sample>)> = Vec::new();
    let mut summaries: Vec<(String, Stats)> = Vec::new();
    for (name, table_name, table) in tables {
        let measured = measure(dataset, &inputs.bed, inputs.width, table);
        for sample in &measured {
            rows.push(record::Row {
                run: inputs.run,
                evaluator: name.to_string(),
                table: table_name.to_string(),
                bed: inputs.bed.name(),
                width: inputs.width,
                seed: sample.seed,
                won: sample.won,
                lines: sample.lines,
                evals: sample.evals,
                ms: sample.ms,
                moves: sample.moves,
                dataset_fingerprint: inputs.dataset_fingerprint.clone(),
            });
        }
        summaries.push((name.to_string(), stats::summarise(&measured)));
        samples.push((name, measured));
    }

    let e1 = &samples[1].1;
    let e2 = &samples[3].1;
    let discordant = stats::paired(e1, e2);
    let ci = stats::paired_bootstrap_ci95(
        e1,
        e2,
        inputs.tune_seed,
        inputs.params.bootstrap_resamples,
    );
    let e1_stats = summaries[1].1.clone();
    let e2_stats = summaries[3].1.clone();
    let win_rate_axis = if e1_stats.saturated() && e2_stats.saturated() {
        "no-resolution"
    } else {
        "resolved"
    };

    let worse = stats::worse_than_published(e1, e2, ci);
    let (decision, rejection_kind, rejection_reason) = match worse {
        Some("win_rate") => (
            "rejected",
            Some("worse_than_published".to_string()),
            Some(format!(
                "E1 wins {} of {} tuning seeds and E2 {} of {}; the exact two-sided McNemar test on the {} discordant pairs gives p = {:.3e} < 0.01",
                e1_stats.wins,
                e1_stats.games,
                e2_stats.wins,
                e2_stats.games,
                discordant.n(),
                stats::mcnemar_p(discordant)
            )),
        ),
        Some("mean_lines") => (
            "rejected",
            Some("worse_than_published".to_string()),
            Some(format!(
                "the win rates tie at {:.4} and E1 clears more lines on average ({:.4} vs {:.4}); the paired bootstrap 95% CI of the difference is [{:.4}, {:.4}], entirely below zero",
                e1_stats.win_rate,
                e1_stats.mean_lines,
                e2_stats.mean_lines,
                ci[0],
                ci[1]
            )),
        ),
        _ => ("accepted", None, None),
    };

    let run = record::Run {
        run: inputs.run,
        date: inputs.date.clone(),
        decision: decision.to_string(),
        rejection_kind,
        rejection_reason,
        bed: record::BedName {
            name: inputs.bed.name(),
            goal: inputs.bed.goal,
            pieces: inputs.bed.pieces,
            cap: inputs.bed.cap,
        },
        bed_source: inputs.bed_source.clone(),
        dataset_fingerprint: inputs.dataset_fingerprint.clone(),
        generator_version: inputs.generator_version,
        width: inputs.width,
        width_curve: inputs.width_curve.clone(),
        split: TrainingSplit::tuning().label(),
        seed_list_fnv: outcome.seed_list_fnv.clone(),
        minibatch_fnv: outcome.minibatch_fnv.clone(),
        tune_seed: format!("0x{:016x}", inputs.tune_seed),
        prng: record::Prng {
            algorithm: "splitmix64".to_string(),
            stream_xor: format!("0x{:016x}", rng::C_TUNE),
            bootstrap_xor: format!("0x{:016x}", rng::C_BOOTSTRAP),
            bounded: "lemire-rejection".to_string(),
            normal: "irwin-hall-12".to_string(),
            draw_order: "iteration, game-seeds (index order, without replacement), candidate, dimension".to_string(),
        },
        params: inputs.params.render(),
        iterations_run: outcome.iterations_run,
        stop_reason: outcome.stop_reason.clone(),
        weights_decimal: outcome.weights_decimal,
        weights_q16: outcome.weights_q16,
        table_fnv: outcome.table_fnv.clone(),
        stats: summaries,
        comparison: record::Comparison {
            e2_vs_e1: record::E2VsE1 {
                b: discordant.b,
                c: discordant.c,
                mcnemar_p: stats::mcnemar_p(discordant),
                win_rate_delta: e2_stats.win_rate - e1_stats.win_rate,
                mean_lines_delta: e2_stats.mean_lines - e1_stats.mean_lines,
                mean_lines_ci95: ci,
                evals_median_delta: e2_stats.evals_median - e1_stats.evals_median,
                evals_p90_delta: e2_stats.evals_p90 - e1_stats.evals_p90,
            },
            win_rate_axis: win_rate_axis.to_string(),
            rule: RULE.to_string(),
        },
        machine: inputs.machine.clone(),
    };
    Ok((run, rows))
}

// ------------------------------------------------------------------ the width

/// §9.5's artifact, read: `DEFAULT_BEAM` comes from the file the page reads, and
/// never from a constant here.
pub fn read_width_curve(path: &Path) -> Result<record::WidthCurveRef, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("read {}: {error}", path.display()))?;
    let value = crate::json::parse(&text)?;
    let default_beam = value
        .get("default_beam")
        .and_then(crate::json::Value::as_u32)
        .ok_or_else(|| format!("{}: default_beam is missing", path.display()))?;
    let basis = value
        .get("default_beam_basis")
        .and_then(crate::json::Value::as_str)
        .unwrap_or("unrecorded")
        .to_string();
    let quality_fingerprint = value
        .get("quality_fingerprint")
        .and_then(crate::json::Value::as_str)
        .unwrap_or("unrecorded")
        .to_string();
    Ok(record::WidthCurveRef {
        path: relative_to_repo(path),
        default_beam,
        default_beam_basis: basis,
        quality_fingerprint,
    })
}

fn relative_to_repo(path: &Path) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("bench/ has a parent");
    path.strip_prefix(root)
        .map(|rest| rest.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"))
}

// -------------------------------------------------------------- the committed file

/// The record, appended to. A retry with a new seed adds a run; nothing is
/// deleted, so the file is the history of how many attempts the table took.
pub fn append_run(path: &Path, run: record::Run) -> Result<record::Record, String> {
    let mut record = match std::fs::read_to_string(path) {
        Ok(text) => record::Record::parse(&text)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => record::Record {
            runs: Vec::new(),
            accepted: None,
        },
        Err(error) => return Err(format!("read {}: {error}", path.display())),
    };
    record.runs.push(run);
    record.accepted = record
        .runs
        .iter()
        .rev()
        .find(|run| run.decision == "accepted")
        .map(|run| record::Accepted {
            table: "E2_TUNED".to_string(),
            run: run.run,
        });
    Ok(record)
}

/// Replace (or create) the rows of one run, leaving other runs' rows alone.
pub fn rewrite_rows(path: &Path, run: u32, rows: &[record::Row]) -> Result<(), String> {
    let mut kept: Vec<String> = Vec::new();
    if let Ok(text) = std::fs::read_to_string(path) {
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let parsed = record::Row::parse(line)?;
            if parsed.run != run {
                kept.push(line.to_string());
            }
        }
    }
    for row in rows {
        kept.push(row.render());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    std::fs::write(path, kept.join("\n") + "\n")
        .map_err(|error| format!("write {}: {error}", path.display()))
}

// ------------------------------------------------------------------- the CLI

/// `bench tune`'s options.
#[derive(Clone, Debug)]
pub struct Options {
    pub data: PathBuf,
    pub tune_seed: u64,
    pub bed: Bed,
    pub params: CeParams,
    pub out: Option<PathBuf>,
    pub rows_out: Option<PathBuf>,
    pub now_secs: u64,
    pub quiet: bool,
    /// `--replay <record>`: re-run the last run's search and check the table.
    pub replay: Option<PathBuf>,
}

impl Options {
    pub fn defaults() -> Options {
        Options {
            data: crate::dataset::root(),
            tune_seed: 0x9E37_79B9_7F4A_7C15,
            bed: Bed::GOAL12,
            params: CeParams::paper(),
            out: None,
            rows_out: None,
            now_secs: 0,
            quiet: false,
            replay: None,
        }
    }

    fn record_path(&self) -> PathBuf {
        self.out.clone().unwrap_or_else(record::record_path)
    }

    fn rows_path(&self) -> PathBuf {
        self.rows_out.clone().unwrap_or_else(record::rows_path)
    }
}

/// What a run did, for the CLI's one line and its exit code.
pub struct RunOutcome {
    pub record: record::Record,
    pub run: u32,
    pub decision: String,
    pub rejection_kind: Option<String>,
    pub rejection_reason: Option<String>,
    pub stats: Vec<(String, Stats)>,
    pub comparison: record::E2VsE1,
    pub win_rate_axis: String,
    pub table_fnv: String,
    pub minibatch_fnv: String,
    pub iterations_run: u32,
    pub stop_reason: String,
    pub wall_clock_seconds: u64,
    pub record_path: PathBuf,
    pub rows_path: PathBuf,
}

/// Measure and commit one run. Exit code 1 on a rejected run, 0 on an accepted
/// one, so a CI job that tried to keep a worse table fails.
pub fn run(options: &Options) -> Result<RunOutcome, String> {
    let started = std::time::Instant::now();
    let split = TrainingSplit::tuning();
    let curve_path = options
        .data
        .parent()
        .expect("data/dataset has a parent: data/")
        .join("measurements")
        .join("width-curve.json");
    let width_curve = read_width_curve(&curve_path)?;

    let (dataset, dataset_fingerprint, generator_version, bed_source) =
        if options.bed.is_shipped() {
            let config = crate::dataset::CONFIGS
                .iter()
                .copied()
                .find(|config| config.dir == options.bed.name())
                .ok_or_else(|| format!("{} is not a committed configuration", options.bed.name()))?;
            let manifest_path = crate::dataset::manifest_path(&options.data, &config);
            let text = std::fs::read_to_string(&manifest_path)
                .map_err(|error| format!("read {}: {error}", manifest_path.display()))?;
            let manifest = crate::dataset::Manifest::parse(&text)?;
            let dataset = TrainingDataset::load(&options.data, &config, &split)?;
            (
                dataset,
                manifest.fingerprint,
                manifest.generator_version,
                "committed-dataset".to_string(),
            )
        } else {
            // §9.1's validation-only bed: never committed, so it is generated
            // from the split's own seeds and its fingerprint is computed here.
            let dataset = TrainingDataset::generate(&options.bed, &split)?;
            let mut fnv = Fnv::new();
            for game in dataset.games() {
                let line = crate::dataset::GameLine::from_engine(game);
                fnv.game(&line);
            }
            (
                dataset,
                crate::fingerprint::hex(fnv.finish()),
                1,
                "generated".to_string(),
            )
        };

    let record_path = options.record_path();
    let rows_path = options.rows_path();
    let next_run = match std::fs::read_to_string(&record_path) {
        Ok(text) => record::Record::parse(&text)?
            .runs
            .iter()
            .map(|run| run.run + 1)
            .max()
            .unwrap_or(0),
        Err(_) => 0,
    };

    let inputs = RunInputs {
        run: next_run,
        date: crate::width::iso_utc(options.now_secs),
        bed: options.bed,
        bed_source,
        dataset_fingerprint,
        generator_version,
        width: width_curve.default_beam,
        width_curve,
        tune_seed: options.tune_seed,
        params: options.params.clone(),
        machine: machine(0),
    };
    let (mut entry, rows) = train(&dataset, &inputs)?;
    entry.machine.wall_clock_seconds = started.elapsed().as_secs();
    let decision = entry.decision.clone();
    let rejection_kind = entry.rejection_kind.clone();
    let rejection_reason = entry.rejection_reason.clone();
    let stats = entry.stats.clone();
    let comparison = entry.comparison.e2_vs_e1.clone();
    let win_rate_axis = entry.comparison.win_rate_axis.clone();
    let table_fnv = entry.table_fnv.clone();
    let minibatch_fnv = entry.minibatch_fnv.clone();
    let iterations_run = entry.iterations_run;
    let stop_reason = entry.stop_reason.clone();

    let record = append_run(&record_path, entry)?;
    if let Some(parent) = record_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    std::fs::write(&record_path, record.render())
        .map_err(|error| format!("write {}: {error}", record_path.display()))?;
    rewrite_rows(&rows_path, next_run, &rows)?;

    Ok(RunOutcome {
        record,
        run: next_run,
        decision,
        rejection_kind,
        rejection_reason,
        stats,
        comparison,
        win_rate_axis,
        table_fnv,
        minibatch_fnv,
        iterations_run,
        stop_reason,
        wall_clock_seconds: started.elapsed().as_secs(),
        record_path,
        rows_path,
    })
}

/// `bench tune --replay <record>`: re-run the committed seed and prove the table
/// is the one that was committed.
pub fn replay(record_path: &Path, data: &Path) -> Result<(String, String, bool), String> {
    let text = std::fs::read_to_string(record_path)
        .map_err(|error| format!("read {}: {error}", record_path.display()))?;
    let record = record::Record::parse(&text)?;
    let run = record
        .runs
        .last()
        .ok_or_else(|| format!("{} has no runs", record_path.display()))?;
    let tune_seed = u64::from_str_radix(run.tune_seed.trim_start_matches("0x"), 16)
        .map_err(|error| format!("tune_seed {:?}: {error}", run.tune_seed))?;
    let bed = Bed {
        goal: run.bed.goal,
        pieces: run.bed.pieces,
        cap: run.bed.cap,
    };
    let split = TrainingSplit::tuning();
    let dataset = if bed.is_shipped() {
        let config = crate::dataset::CONFIGS
            .iter()
            .copied()
            .find(|config| config.dir == bed.name())
            .ok_or_else(|| format!("{} is not a committed configuration", bed.name()))?;
        TrainingDataset::load(data, &config, &split)?
    } else {
        TrainingDataset::generate(&bed, &split)?
    };
    let params = CeParams::paper();
    let outcome = search(&dataset, &bed, run.width, &params, tune_seed)?;
    let same = outcome.table_fnv == run.table_fnv && outcome.minibatch_fnv == run.minibatch_fnv;
    Ok((run.table_fnv.clone(), outcome.table_fnv, same))
}

// ------------------------------------------------------------------- machine

/// What the run was measured on, so a moved table is detectable rather than
/// mysterious. A command that is missing is recorded as such; nothing here
/// decides a number.
fn machine(wall_clock_seconds: u64) -> record::Machine {
    record::Machine {
        engine_commit: git(&["rev-parse", "--short", "HEAD"]),
        bench_commit: git(&["rev-parse", "--short", "HEAD"]),
        rustc: command("rustc", &["--version"]),
        target: format!(
            "{}-{}",
            std::env::consts::ARCH,
            std::env::consts::OS
        ),
        cpu: std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_| "unrecorded".to_string()),
        os: std::env::consts::OS.to_string(),
        profile: "release: lto=fat, codegen-units=1, panic=abort, strip=true".to_string(),
        wall_clock_seconds,
    }
}

fn command(program: &str, args: &[&str]) -> String {
    std::process::Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_else(|| "unrecorded".to_string())
}

fn git(args: &[&str]) -> String {
    command("git", args)
}

/// The engine's own `Board`/`PlacementCtx` re-exported for the tests that build a
/// fixture board; keeps the test imports to this module.
pub fn fixture_board() -> (Board, PlacementCtx) {
    (Board([0u32; 10]), PlacementCtx::NONE)
}
