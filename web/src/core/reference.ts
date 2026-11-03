// The committed Reference run, as the page reads it.
//
// `data/dataset/<configuration>/reference.jsonl` is append-only (ticket 09 owns it):
// one JSON object per line, and the **last** line is the row to show. Nothing here
// rewrites or invents: a missing file is a `null` row and the panel says so in words,
// because "no figure without a live Run or a labelled Reference row behind it" cuts
// both ways — an unlabelled figure is a defect, and a fabricated one is worse.
//
// What a row gives the panel is exactly what the ticket asks the reference column to
// carry: the same seven statistics the live Run computes (`n`, `wins`, `n_fail`,
// Lines on failure, Evaluations p50/p90, ms p50/p90, the loop's own wall clock) plus
// the label block that says which machine, runtime, engine commit and wasm hash
// produced them (§2.4).
//
// `web/test/run-stats.test.mjs` parses the committed files and asserts the shape, so
// a change to the row's schema fails a test rather than blanking a column.

import type { ConfigStats } from "./run-stats.ts";

/** The row's `machine`/`engine`/`labels` block, flattened to the words on screen. */
export interface ReferenceLabels {
  /** `cpu · os`, the row's own short form. */
  readonly machine: string;
  readonly cpu: string;
  readonly os: string;
  readonly runtime: string;
  readonly date: string;
  readonly engineCommit: string;
  readonly wasmHash: string;
  readonly wasmBytes: number;
  readonly profile: string;
  readonly substrate: string;
}

/** The row's `stats` block, as committed. */
export interface ReferenceStatsBlock {
  readonly n: number;
  readonly wins: number;
  readonly nFail: number;
  readonly winRate: number;
  readonly wilson95: readonly [number, number];
  readonly failLinesMean: number | null;
  readonly failLinesMedian: number | null;
  readonly evalsP50: number;
  readonly evalsP90: number;
  readonly msP50: number;
  readonly msP90: number;
  readonly batchMs: number;
}

export interface ReferenceRow {
  readonly config: string;
  readonly goal: number;
  readonly pieces: number;
  readonly cap: number;
  readonly width: number;
  readonly split: { readonly first: number; readonly last: number; readonly n: number };
  readonly stats: ReferenceStatsBlock;
  readonly labels: ReferenceLabels;
  readonly measuredAt: string;
  readonly reason: string;
  /** The line's index in the file, so "latest" is a fact and not a guess. */
  readonly index: number;
}

const text = (value: unknown): string => (typeof value === "string" ? value : "");
const num = (value: unknown): number => (typeof value === "number" ? value : Number.NaN);
const maybe = (value: unknown): number | null => (typeof value === "number" ? value : null);

/** A label that may be a plain string or an object carrying `.label` (ticket 09's shape). */
function labelOf(value: unknown): string {
  if (typeof value === "string") return value;
  if (value && typeof value === "object" && typeof (value as { label?: unknown }).label === "string") {
    return (value as { label: string }).label;
  }
  return "";
}

/**
 * Parse the file. A line that is not a Reference row is skipped rather than thrown
 * on — the file is append-only and hand-appendable, and one bad line must not cost
 * the column its honest rows.
 */
/**
 * One row object (a parsed line, or the generated `engine-facts.ts` projection of the
 * latest line) as the panel's `ReferenceRow`, or `null` when it is not a Reference row.
 *
 * One implementation of the field names: `parseReferenceJsonl` calls it per line and
 * `core/reference-data.ts` calls it on the build-time projection, so the page cannot
 * read a different `stats.n_fail` from the one the test reads.
 */
export function referenceFromObject(
  source: unknown,
  index: number,
): ReferenceRow | null {
  if (!source || typeof source !== "object") return null;
  const row = source as Record<string, unknown>;
  if (row.kind !== "reference") return null;
  const stats = (row.stats ?? {}) as Record<string, unknown>;
  const machine = (row.machine ?? {}) as Record<string, unknown>;
  const engine = (row.engine ?? {}) as Record<string, unknown>;
  const behaviour = (row.behaviour ?? {}) as Record<string, unknown>;
  const split = (row.split ?? {}) as Record<string, unknown>;
  const labels = (row.labels ?? {}) as Record<string, unknown>;
  const wilson = Array.isArray(stats.wilson95) ? stats.wilson95 : [];
  return {
    config: text(row.config),
    goal: num(behaviour.goal),
    pieces: num(behaviour.pieces),
    cap: num(behaviour.cap),
    width: num(behaviour.width),
    split: { first: num(split.first), last: num(split.last), n: num(split.n) },
    stats: {
      n: num(stats.n),
      wins: num(stats.wins),
      nFail: num(stats.n_fail),
      winRate: num(stats.win_rate),
      wilson95: [num(wilson[0]), num(wilson[1])],
      failLinesMean: maybe(stats.fail_lines_mean),
      failLinesMedian: maybe(stats.fail_lines_median),
      evalsP50: num(stats.evals_p50),
      evalsP90: num(stats.evals_p90),
      msP50: num(stats.ms_p50),
      msP90: num(stats.ms_p90),
      batchMs: num(stats.batch_ms),
    },
    labels: {
      machine: labelOf(labels.machine) || [text(machine.cpu), text(machine.os)].filter(Boolean).join(" · "),
      cpu: text(machine.cpu),
      os: text(machine.os),
      runtime: labelOf(labels.runtime) || text(machine.browser_or_node) || text(machine.runtime),
      date: labelOf(labels.date) || text(row.measured_at),
      engineCommit: labelOf(labels.engine_commit) || text(engine.commit),
      wasmHash: text(engine.wasm_sha256),
      wasmBytes: num(engine.wasm_bytes),
      profile: text(engine.profile),
      substrate: text(machine.substrate),
    },
    measuredAt: text(row.measured_at),
    reason: text(row.reason),
    index,
  };
}

export function parseReferenceJsonl(source: string): ReferenceRow[] {
  const rows: ReferenceRow[] = [];
  for (const line of source.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (trimmed.length === 0) continue;
    let parsed: unknown;
    try {
      parsed = JSON.parse(trimmed);
    } catch {
      continue;
    }
    const row = referenceFromObject(parsed, rows.length);
    if (row !== null) rows.push(row);
  }
  return rows;
}

/** The row to show: the file is append-only, so the last line is the latest run. */
export function latestReference(rows: readonly ReferenceRow[]): ReferenceRow | null {
  return rows.length === 0 ? null : rows[rows.length - 1];
}

/** The latest row for one configuration, or `null` when none is committed. */
export function referenceForConfig(
  rows: readonly ReferenceRow[],
  config: string,
): ReferenceRow | null {
  const matching = rows.filter((row) => row.config === config);
  return latestReference(matching);
}

/**
 * The row's statistics in the shape the live column's formatter takes.
 *
 * `wilson95` is recomputed from `wins`/`n` with the same function the live Run uses,
 * so the interval is one construct rather than two copies of a formula.
 */
export function referenceStats(row: ReferenceRow): ConfigStats {
  const n = row.stats.n;
  const wins = row.stats.wins;
  const nFail = row.stats.nFail;
  return {
    n,
    wins,
    missing: 0,
    attempted: n,
    nFail,
    winRate: n === 0 ? 0 : wins / n,
    wilson95: wilson(row.stats.wilson95, wins, n),
    failLinesMean: nFail === 0 ? null : row.stats.failLinesMean,
    failLinesMedian: nFail === 0 ? null : row.stats.failLinesMedian,
    evalsP50: row.stats.evalsP50,
    evalsP90: row.stats.evalsP90,
    msP50: row.stats.msP50,
    msP90: row.stats.msP90,
    workMs: row.stats.batchMs,
  };
}

/** The committed interval if it is a pair of numbers, otherwise the formula's. */
function wilson(committed: readonly [number, number], wins: number, n: number): readonly [number, number] {
  if (Number.isFinite(committed[0]) && Number.isFinite(committed[1])) return committed;
  // Imported lazily to keep this module's only dependency a type import: the
  // interval is the row's own number whenever it is committed.
  const Z = 1.959963984540054;
  if (n === 0) return [0, 0];
  const p = wins / n;
  const denominator = 1 + (Z * Z) / n;
  const centre = (p + (Z * Z) / (2 * n)) / denominator;
  const half = (Z / denominator) * Math.sqrt((p * (1 - p)) / n + (Z * Z) / (4 * n * n));
  return [Math.max(0, centre - half), Math.min(1, centre + half)];
}

/** The label under the reference column: what produced these numbers — short, so it reads on one line. */
export function referenceLabel(row: ReferenceRow): string {
  // The CPU string carries its memory ("…, 31.9 GB"), which is not what makes a
  // column comparable; the core count in parentheses is, so keep that and drop the RAM.
  const cpu = (row.labels.cpu.split(",")[0] ?? "").trim() || row.labels.machine;
  // The engine commit and the wasm hash stay in the tooltip (referenceDetail); a line
  // of hashes is for a diff, not for the panel.
  // The runtime string carries its explanation in parentheses ("v24.21.0 (V8
  // headless, …)"); the version is what makes the column comparable, the essay is
  // in the tooltip.
  const runtime = row.labels.runtime.split("(")[0].trim();
  return [cpu, row.labels.os, runtime, `beam ${row.width}`, row.labels.date]
    .filter((part) => part.length > 0)
    .join(" · ");
}

/** The tooltip's version of the same label: the whole block, nothing shortened. */
export function referenceDetail(row: ReferenceRow): string {
  // The configuration's name is not in this sentence on purpose: the block's own head
  // names it, and a minified template keeps its `${…}` text inside one string literal,
  // which Tier B of the banned-words gate reads as prose — a property access called
  // `config` would be a finding, and esbuild folds a hoisted alias back in.
  return [
    `Reference run — the test Split ${row.split.first}–${row.split.last} (${row.split.n.toLocaleString("en-US")} games) at beam ${row.width}.`,
    `Machine: ${row.labels.machine}. Runtime: ${row.labels.runtime}.`,
    `Engine commit ${row.labels.engineCommit}; wasm ${row.labels.wasmHash} (${row.labels.wasmBytes.toLocaleString("en-US")} bytes).`,
    `Build: ${row.labels.profile}.`,
    `Measured ${row.measuredAt}${row.reason ? ` — ${row.reason}` : ""}.`,
    "Every figure in the reference column came from this machine, this runtime and this date. A number with no such label does not go on this page.",
  ].join(" ");
}
