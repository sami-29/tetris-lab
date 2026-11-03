// The Run's figures, from the engine's own integers.
//
// Everything the Run panel shows is one of three things, and this file is where
// that is enforced rather than described:
//
//   (a) a count of games the engine answered — §4.3's stride-4 summary words
//       `won, lines, evaluations, status`;
//   (b) a rank statistic over those integers, computed the way `bench reference`
//       computes the Reference row's (`nearest-rank`, `k = ceil(p·n)`, no
//       interpolation — notes/09 §1.2), so the two columns are one construct and
//       not two;
//   (c) the one measurement the engine cannot make: time, which the worker reads
//       from `performance.now()` around each `solve_batch` call because
//       `std::time::Instant::now()` traps on `wasm32-unknown-unknown`.
//
// Two absences are load-bearing:
//
//   * a game whose `status` is not 0 is **missing**, never a loss. It contributes
//     no failure sample, no percentile sample and no win. A `0 Lines` failure or a
//     `0 Evaluations` percentile would be a figure with no measurement behind it;
//   * a bed with no failures has a `null` mean and median Lines on failure, which
//     renders as `no failures` — never `0`.
//
// `won` is the summary's own first word, never `lines >= goal`: notes/09 measured a
// win clearing nine Lines against a Goal of eight, so the arithmetic is not the
// rule.
//
// Pure data in, pure data out: no DOM, no wasm, no clock.

/** One committed configuration's behaviour and its test split. */
export interface ConfigShape {
  /** 0 or 1: the two committed configurations, in the panel's order. */
  readonly id: 0 | 1;
  /** The Dataset directory's name, e.g. `goal8-p40-cap7`. */
  readonly name: string;
  readonly goal: number;
  readonly pieces: number;
  readonly cap: number;
  readonly beamWidth: number;
  /** The test split's first and last Seed, inclusive. */
  readonly first: number;
  readonly last: number;
}

/** What one configuration has accumulated so far. Built only from `ChunkMessage`s. */
export interface ConfigTotals {
  readonly shape: ConfigShape;
  /** Games the engine answered (`status` 0). */
  n: number;
  wins: number;
  /** Games that produced no measurement (`status` != 0, or a lost chunk). */
  missing: number;
  /** Lines of the measured losses, unsorted. */
  failLines: number[];
  /** `evaluations` of the measured games, unsorted. */
  evals: number[];
  /** Per-game wall clock in microseconds, measured in the worker, measured games only. */
  micros: number[];
  /** Σ of the chunks' own measured work time, in ms: the same quantity as `batch_ms`. */
  workMs: number;
}

export function emptyTotals(shape: ConfigShape): ConfigTotals {
  return { shape, n: 0, wins: 0, missing: 0, failLines: [], evals: [], micros: [], workMs: 0 };
}

/**
 * Fold one applied chunk into the totals.
 *
 * `summaries` is §4.3's stride-4 array and `micros` is one `u32` per game, both
 * transferred from the worker. A `status` that is not 0 contributes to `missing`
 * and to nothing else.
 */
export function addSummaries(
  totals: ConfigTotals,
  summaries: Uint32Array,
  micros: Uint32Array,
  workMs: number,
): void {
  const games = Math.floor(summaries.length / 4);
  for (let index = 0; index < games; index += 1) {
    const at = index * 4;
    const won = summaries[at];
    const lines = summaries[at + 1];
    const evaluations = summaries[at + 2];
    const status = summaries[at + 3];
    if (status !== 0) {
      totals.missing += 1;
      continue;
    }
    totals.n += 1;
    totals.evals.push(evaluations);
    totals.micros.push(index < micros.length ? micros[index] : 0);
    if (won === 1) totals.wins += 1;
    else totals.failLines.push(lines);
  }
  totals.workMs += workMs;
}

/** Games a chunk could not answer at all (a trap that survived its retry). */
export function addMissing(totals: ConfigTotals, games: number): void {
  totals.missing += games;
}

/** The seven rows' numbers, once the Run has stopped changing. */
export interface ConfigStats {
  readonly n: number;
  readonly wins: number;
  readonly missing: number;
  /** `n + missing`: the games the Run asked for. */
  readonly attempted: number;
  readonly nFail: number;
  readonly winRate: number;
  readonly wilson95: readonly [number, number];
  readonly failLinesMean: number | null;
  readonly failLinesMedian: number | null;
  readonly evalsP50: number;
  readonly evalsP90: number;
  readonly msP50: number;
  readonly msP90: number;
  readonly workMs: number;
}

/**
 * Nearest rank on a sorted array: `k = ceil(num/den · n)`, value `x[k-1]`.
 *
 * `null` for no samples, which is why a percentile over an empty bed can never be
 * printed as `0`. `bench/src/width.rs::nearest_rank` is the same rule, so a live
 * Run's p90 and a Reference row's p90 are the same statistic.
 */
export function percentile(sorted: readonly number[], num: number, den: number): number | null {
  if (sorted.length === 0) return null;
  const rank = Math.ceil((num / den) * sorted.length);
  return sorted[Math.min(sorted.length, Math.max(1, rank)) - 1];
}

/**
 * Wilson's score interval, 95 %, no continuity correction — `bench/src/reference.rs`
 * verbatim, including the two exact ends: `p = 1` gives 1 and not 1 − 2e-19.
 */
export function wilson95(wins: number, n: number): [number, number] {
  const Z = 1.959963984540054;
  if (n === 0) return [0, 0];
  const p = wins / n;
  const denominator = 1 + (Z * Z) / n;
  const centre = (p + (Z * Z) / (2 * n)) / denominator;
  const half = (Z / denominator) * Math.sqrt((p * (1 - p)) / n + (Z * Z) / (4 * n * n));
  let lower = Math.min(1, Math.max(0, centre - half));
  let upper = Math.min(1, Math.max(0, centre + half));
  if (wins === 0) lower = 0;
  if (wins >= n) upper = 1;
  return [lower, upper];
}

const ascending = (a: number, b: number): number => a - b;

/** `ConfigTotals` in, the seven rows' numbers out. Sorts copies, never the totals. */
export function statsOf(totals: ConfigTotals): ConfigStats {
  const failLines = [...totals.failLines].sort(ascending);
  const evals = [...totals.evals].sort(ascending);
  const ms = totals.micros.map((micro) => micro / 1000).sort(ascending);
  const n = totals.n;
  const mean = failLines.length
    ? failLines.reduce((sum, lines) => sum + lines, 0) / failLines.length
    : null;
  return {
    n,
    wins: totals.wins,
    missing: totals.missing,
    attempted: n + totals.missing,
    nFail: failLines.length,
    winRate: n === 0 ? 0 : totals.wins / n,
    wilson95: wilson95(totals.wins, n),
    failLinesMean: mean,
    failLinesMedian: percentile(failLines, 1, 2),
    evalsP50: percentile(evals, 1, 2) ?? 0,
    evalsP90: percentile(evals, 9, 10) ?? 0,
    msP50: percentile(ms, 1, 2) ?? 0,
    msP90: percentile(ms, 9, 10) ?? 0,
    workMs: totals.workMs,
  };
}

/** The panel's words for a bed with no losses. Never `0`. */
export const FAILURES_NONE = "no failures";

/** One cell per figure row, already in the panel's words. */
export interface RunCells {
  readonly winRate: string;
  readonly gamesWon: string;
  readonly failLinesMean: string;
  readonly failLinesMedian: string;
  readonly evals: string;
  readonly ms: string;
  readonly total: string;
}

export interface CellOptions {
  /**
   * The live column. It always carries the denominator it actually measured
   * (`100.0 % of 1,000`), because a win rate without its `n` is a claim about a
   * number of games nobody stated — and a Run can be stopped at any count. The
   * Reference column passes neither this nor `clock`: its label block says it is the
   * whole split.
   */
  readonly live?: boolean;
  /** A Run that was stopped: every cell then carries its own denominator. */
  readonly partial?: boolean;
  /**
   * The clock's quantisation in ms, when the numbers came from this browser
   * (`performance.now()` is coarsened to 100 µs in a non-cross-origin-isolated
   * context, which §7's no-COOP/COEP rule makes every Run).
   */
  readonly clock?: number;
  /** The main thread's own wall clock, which includes the message hops. */
  readonly elapsedMs?: number;
}

const group = (value: number): string => Math.round(value).toLocaleString("en-US");
const secs = (ms: number): string => (ms / 1000).toFixed(1);

/**
 * The seven cells, from one `ConfigStats`.
 *
 * One formatter for both columns: the live column passes `partial`/`clock`, the
 * Reference column passes neither, and the arithmetic above is identical — so
 * "beside the Reference run's same rows" is a comparison of like with like, and the
 * only difference on screen is the label that names the machine.
 */
export function formatCells(stats: ConfigStats, options: CellOptions = {}): RunCells | null {
  // Nothing measured is not a row of zeros: the slot keeps its placeholder words.
  if (stats.n === 0 && stats.missing === 0) return null;

  const pct = `${(stats.winRate * 100).toFixed(1)} %`;
  const failLines = (value: number | null): string =>
    value === null ? FAILURES_NONE : `${value.toFixed(1).replace(/\.0$/, "")} (n = ${group(stats.nFail)})`;
  const pair = (p50: number, p90: number, suffix = ""): string =>
    `${group(p50)}/${group(p90)}${suffix}`;
  // Milliseconds keep one decimal: a Solve at the default width is ~2.5 ms, and
  // rounding it to a whole ms would throw away most of the number.
  const msPair = (p50: number, p90: number): string => `${p50.toFixed(1)}/${p90.toFixed(1)}`;

  // A cell is the live column's when it carries a fact only this tab can have: its
  // clock, its own wall clock, or the fact that it was stopped.
  const live = options.live ?? (options.partial === true || options.clock !== undefined || options.elapsedMs !== undefined);
  const n = options.partial ? ` (n = ${group(stats.n)})` : "";
  const clock = options.clock !== undefined ? ` ±${options.clock}` : "";
  const total = options.partial
    ? `${secs(stats.workMs)} s work (stopped)`
    : options.elapsedMs !== undefined
      ? `${secs(stats.workMs)} s work · ${secs(options.elapsedMs)} s elapsed`
      : `${secs(stats.workMs)} s this configuration`;

  return {
    winRate: live ? `${pct} of ${group(stats.n)}` : pct,
    gamesWon:
      stats.missing > 0
        ? `${group(stats.wins)} of ${group(stats.n)} · ${group(stats.missing)} missing`
        : `${group(stats.wins)} of ${group(stats.n)}`,
    failLinesMean: failLines(stats.failLinesMean),
    failLinesMedian: failLines(stats.failLinesMedian),
    evals: pair(stats.evalsP50, stats.evalsP90, n),
    ms: `${msPair(stats.msP50, stats.msP90)} ms${clock}`,
    total,
  };
}
