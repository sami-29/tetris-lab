// Seam 2: the Run's figures, computed from engine integers and nothing else.
//
// This is the file that makes the ticket's last rule checkable: **no figure on
// screen without a live Run or a labelled Reference row behind it.** Every row the
// Run panel shows is one of three things —
//
//   (a) a count of games the engine answered (`won`, `lines`, `evaluations`, `status`
//       words from §4.3's stride-4 summary),
//   (b) a rank statistic over those integers, computed the way `bench` computes the
//       Reference row's (`nearest-rank`, no interpolation — notes/09 §1.2), or
//   (c) the one measurement the engine cannot make: time, which the worker reads
//       from `performance.now()` because `Instant::now()` traps in wasm.
//
// Two things are deliberately asserted as *absences*: a game with a non-zero
// `status` is **missing**, never a loss (S4), and a bed with no failures prints
// `no failures`, never `0` (S3). Both are the fabrications this ticket exists to
// forbid — a `0 lines` failure sample and a `0 evaluations` percentile would both be
// figures with no measurement behind them.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join, resolve } from "node:path";

import {
  FAILURES_NONE,
  addMissing,
  addSummaries,
  emptyTotals,
  formatCells,
  percentile,
  statsOf,
  wilson95,
} from "../src/core/run-stats.ts";
import {
  latestReference,
  parseReferenceJsonl,
  referenceDetail,
  referenceFromObject,
  referenceLabel,
  referenceStats,
} from "../src/core/reference.ts";
import { ENGINE_FACTS } from "../src/generated/engine-facts.ts";
import { TIER_A, TIER_B, scanText } from "../scripts/words.mjs";
import { ASSUMPTION, partialNote, yoursLabel } from "../src/ui/run-copy.ts";

const ROOT = resolve(fileURLToPath(new URL(".", import.meta.url)), "..", "..");
const read = (...parts) => readFileSync(join(ROOT, ...parts), "utf8");

const SHAPE = {
  id: 0,
  name: "goal8-p40-cap7",
  goal: 8,
  pieces: 40,
  cap: 7,
  beamWidth: 25,
  first: 0,
  last: 999,
};

/** Build a summary array the way §4.3 lays one out: won, lines, evaluations, status. */
function summaries(games) {
  const words = new Uint32Array(games.length * 4);
  games.forEach(([won, lines, evaluations, status], index) => {
    words.set([won, lines, evaluations, status], index * 4);
  });
  return words;
}

const micros = (count, value) => Uint32Array.from({ length: count }, () => value);

test("S1: nearest rank, no interpolation — p50 is x[n/2 - 1], p90 is x[ceil(0.9n) - 1]", () => {
  const thousand = Array.from({ length: 1000 }, (_, index) => index);
  assert.equal(percentile(thousand, 1, 2), thousand[499], "1000 values: p50 = x[499]");
  assert.equal(percentile(thousand, 9, 10), thousand[899], "1000 values: p90 = x[899]");

  const twentyFive = Array.from({ length: 25 }, (_, index) => index * 10);
  assert.equal(percentile(twentyFive, 1, 2), twentyFive[12], "25 values: p50 = x[12]");
  assert.equal(percentile(twentyFive, 9, 10), twentyFive[22], "25 values: p90 = x[22]");

  assert.equal(percentile([], 1, 2), null, "no samples is null, never 0");
});

test("S2: the Wilson interval is notes/09 §1.2's, reproduced at both ends", () => {
  const [saturatedLow, saturatedHigh] = wilson95(1000, 1000);
  assert.equal(saturatedLow.toFixed(6), "0.996173", "1000/1000 lower bound");
  assert.equal(saturatedHigh, 1, "and exactly 1 at the top, not 1 - 2e-19");

  const [low, high] = wilson95(995, 1000);
  assert.equal(low.toFixed(6), "0.988349", "995/1000 lower bound");
  assert.equal(high.toFixed(6), "0.997862", "995/1000 upper bound");

  assert.deepEqual(wilson95(0, 0), [0, 0], "no games, no interval");
});

test("S3: no failures prints 'no failures', never 0", () => {
  const totals = emptyTotals(SHAPE);
  addSummaries(
    totals,
    summaries([
      [1, 8, 9000, 0],
      [1, 9, 9100, 0],
      [1, 8, 9200, 0],
    ]),
    micros(3, 2000),
    6,
  );
  const stats = statsOf(totals);
  assert.equal(stats.n, 3);
  assert.equal(stats.nFail, 0);
  assert.equal(stats.failLinesMean, null, "a mean over no losses is null");
  assert.equal(stats.failLinesMedian, null, "and so is the median");

  const cells = formatCells(stats);
  assert.equal(cells.failLinesMean, FAILURES_NONE);
  assert.equal(cells.failLinesMedian, FAILURES_NONE);
  assert.equal(FAILURES_NONE, "no failures", "the words, not a dash");
});

test("S4: a non-zero status is a missing game, never a loss and never a sample", () => {
  const totals = emptyTotals(SHAPE);
  // Eight games: six measured (five wins, one loss), two that produced no answer.
  addSummaries(
    totals,
    summaries([
      [1, 8, 1000, 0],
      [1, 8, 1100, 0],
      [1, 9, 1200, 0],
      [1, 8, 1300, 0],
      [1, 8, 1400, 0],
      [0, 3, 1500, 0],
      [0, 0, 0, 2],
      [0, 0, 0, 1],
    ]),
    micros(8, 3000),
    24,
  );
  const stats = statsOf(totals);
  assert.equal(stats.n, 6, "six games were measured");
  assert.equal(stats.wins, 5);
  assert.equal(stats.missing, 2, "two produced no measurement");
  assert.equal(stats.nFail, 1, "one loss — the two status rows are not losses");
  assert.equal(stats.failLinesMean, 3, "the loss is the only failure sample");
  // Six measured games, nearest rank: k = ceil(0.5 * 6) = 3, so p50 is the third.
  assert.equal(stats.evalsP50, 1200, "the percentiles are over the six measured games");

  const cells = formatCells(stats, { partial: true });
  assert.match(cells.gamesWon, /^5 of 6 · 2 missing$/, cells.gamesWon);
  assert.match(cells.winRate, /^83\.3 % of 6$/, cells.winRate);
});

test("S5: `won` is the engine's word, not `lines >= goal`", () => {
  // notes/09 §1.2 measured a seed clearing nine Lines against a Goal of eight: a win
  // whose `lines` exceeds the Goal. Deriving `won` from the arithmetic would also
  // turn a *loss* the engine reported into a win, so the summary's first word wins.
  const totals = emptyTotals(SHAPE);
  addSummaries(
    totals,
    summaries([
      [1, 9, 5000, 0], // a win with lines > goal
      [0, 8, 5000, 0], // the engine says lost, even though lines >= goal
    ]),
    micros(2, 2000),
    4,
  );
  const stats = statsOf(totals);
  assert.equal(stats.wins, 1, "one win: the summary's own word");
  assert.equal(stats.nFail, 1);
  assert.equal(stats.failLinesMean, 8, "the reported loss contributes its 8 Lines");
});

test("S6: a chunk is applied once, and a lost chunk is counted as missing", () => {
  const totals = emptyTotals(SHAPE);
  addSummaries(totals, summaries([[1, 8, 100, 0], [1, 8, 200, 0]]), micros(2, 1000), 2);
  addMissing(totals, 3);
  const stats = statsOf(totals);
  assert.equal(stats.n, 2);
  assert.equal(stats.missing, 3, "a trapped chunk's games are missing, not losses");
  assert.equal(stats.attempted, 5);
  assert.equal(totals.workMs, 2);
});

test("S7: the cells a complete Run shows, in the panel's own words", () => {
  const totals = emptyTotals(SHAPE);
  const games = [];
  for (let seed = 0; seed < 1000; seed += 1) games.push([1, 8 + (seed % 2), 9000 + seed, 0]);
  addSummaries(totals, summaries(games), micros(1000, 2500), 2500);
  const stats = statsOf(totals);
  assert.equal(stats.n, 1000);
  assert.equal(stats.wins, 1000);
  assert.equal(stats.evalsP50, 9499, "x[499] of 9000…9999");
  assert.equal(stats.evalsP90, 9899, "x[899]");
  assert.equal(stats.msP50, 2.5, "µs are reported as ms");
  assert.equal(stats.msP90, 2.5);

  const cells = formatCells(stats, { clock: 0.1, elapsedMs: 3000 });
  assert.equal(cells.winRate, "100.0 % of 1,000");
  assert.equal(cells.gamesWon, "1,000 of 1,000");
  assert.equal(cells.evals, "9,499/9,899");
  assert.equal(cells.ms, "2.5/2.5 ms ±0.1");
  assert.equal(cells.total, "2.5 s work · 3.0 s elapsed");
});

test("S8: a stopped Run shows its denominator in the same cell as its value", () => {
  const totals = emptyTotals(SHAPE);
  const games = [];
  for (let seed = 0; seed < 640; seed += 1) games.push([seed % 10 === 0 ? 0 : 1, 8, 9000 + seed, 0]);
  addSummaries(totals, summaries(games), micros(640, 2000), 700);
  const cells = formatCells(statsOf(totals), { partial: true, clock: 0.1 });
  assert.equal(cells.winRate, "90.0 % of 640", "a win rate without its n is a claim about 1,000 games");
  assert.equal(cells.evals, "9,319/9,575 (n = 640)");
  assert.equal(cells.total, "0.7 s work (stopped)");
  assert.equal(cells.ms, "2.0/2.0 ms ±0.1");
});

test("S9: no games measured yet is no cells at all, so the slot keeps its words", () => {
  const cells = formatCells(statsOf(emptyTotals(SHAPE)));
  assert.equal(cells, null, "the renderer prints its placeholder instead of a 0");
});

test("S10: the committed Reference rows parse, and the latest one per configuration wins", () => {
  for (const name of ["goal8-p40-cap7", "goal12-p100-cap7"]) {
    const rows = parseReferenceJsonl(read("data", "dataset", name, "reference.jsonl"));
    assert.ok(rows.length >= 1, `${name}: a row is committed`);
    const latest = latestReference(rows);
    assert.equal(latest.config, name);
    assert.equal(latest.split.n, 1000, "the test split, all of it");
    assert.equal(latest.split.first, 0);
    assert.equal(latest.split.last, 999);
    assert.ok(latest.labels.machine.length > 3, "the row names its machine");
    assert.ok(latest.labels.runtime.length > 3, "and its runtime");
    assert.match(latest.labels.engineCommit, /^[0-9a-f]{7,40}$/, "and its engine commit");

    const stats = referenceStats(latest);
    assert.equal(stats.n, 1000);
    assert.equal(stats.wins, 1000);
    assert.ok(stats.evalsP50 > 1000 && stats.evalsP90 >= stats.evalsP50);
    assert.ok(stats.msP50 > 0 && stats.msP90 >= stats.msP50);
    assert.ok(stats.workMs > 0, "the row carries the loop's own wall clock");
  }
});

test("S11: both columns are formatted by one formatter, so they cannot be two statistics", () => {
  const rows = parseReferenceJsonl(read("data", "dataset", "goal8-p40-cap7", "reference.jsonl"));
  const latest = latestReference(rows);
  const cells = formatCells(referenceStats(latest));
  assert.equal(cells.winRate, "100.0 %", "the reference cell carries no denominator — its label block does");
  assert.match(cells.evals, /^\d[\d,]*\/\d[\d,]*$/, cells.evals);
  assert.match(cells.ms, /^[\d.]+\/[\d.]+ ms$/, cells.ms);
  assert.match(cells.total, /^[\d.]+ s this configuration$/, cells.total);

  const label = referenceLabel(latest);
  assert.match(label, /Ryzen/, "the label names the CPU");
  assert.match(label, /Windows/, "and the OS");
  assert.match(label, /beam 25/, "and the beam width the row was measured at");
  // The commit and the wasm hash moved to the tooltip (referenceDetail): a line of
  // hashes is for a diff, not for the panel.
  assert.doesNotMatch(label, /wasm\s|[0-9a-f]{7,40}/, "the label stays short");
  assert.ok(referenceDetail(latest).includes(latest.labels.engineCommit), "the detail still names the commit");
});

test("S12: the mid-range 8-core laptop is an assumption, and the page says so in those words", () => {
  assert.match(ASSUMPTION, /assumption/);
  assert.match(ASSUMPTION, /not a measurement/);
  assert.match(ASSUMPTION, /8-core laptop/);
  assert.match(ASSUMPTION, /5 s|five seconds/);
});

test("S13: the `yours` label names what a browser can actually know", () => {
  const label = yoursLabel({ workers: 8, cores: 16, beam: 25, browser: "Chrome 141", os: "Windows" });
  assert.match(label, /8 workers of 16 logical cores/);
  assert.match(label, /beam 25/);
  assert.match(label, /Chrome 141/);
  assert.match(label, /this browser/, "the substrate is part of the label");
  // A page cannot read the CPU brand string, so it must not print one.
  assert.doesNotMatch(label, /Ryzen|Intel|Apple M/);
});

test("S14: a partial Run says partial in words, and never compares itself to the reference", () => {
  const note = partialNote({ games: 640, total: 2000 });
  assert.match(note, /stopped/);
  assert.match(note, /640 of 2,000/);
  assert.match(note, /partial/);
  assert.doesNotMatch(note, /faster|slower|beats|than the reference/i, "a prefix is not a comparison");
});

test("S15: the generated Reference rows are the committed rows, less one Rust keyword", () => {
  // The page reads `ENGINE_FACTS.reference.rows`, projected from `reference.jsonl` at
  // build time; the tests read the file. This is what keeps the two one measurement:
  // the projection is the latest committed row verbatim, and the only edit anywhere is
  // the Rust `target` keyword in the build label (the glossary bans `target`).
  assert.ok(ENGINE_FACTS.reference.rows.length >= 1, "a Reference row is committed");
  for (const fact of ENGINE_FACTS.reference.rows) {
    const lines = read("data", "dataset", fact.config, "reference.jsonl")
      .split(/\r?\n/)
      .filter((line) => line.trim().length > 0);
    assert.ok(lines.length >= 1, `${fact.config}: the file has a row`);
    const latest = JSON.parse(lines.at(-1));
    const expected = structuredClone(latest);
    expected.engine.profile = expected.engine.profile.replace(/\btarget\s+/gi, "");
    assert.deepEqual(fact.row, expected, `${fact.config}: the projection is the latest committed row`);

    const generated = referenceFromObject(fact.row, 0);
    const parsed = referenceFromObject(latest, 0);
    assert.deepEqual(referenceStats(generated), referenceStats(parsed), "one set of Statistics, two reads");
    assert.equal(referenceLabel(generated), referenceLabel(parsed), "one label, two reads");
    assert.match(referenceDetail(generated), /Reference run/, "the detail names the row");
  }
});

test("S16: nothing the Reference column ships says a word the page forbids", () => {
  // `check-words.mjs` scans the built bundle, which cannot tell an embedded artifact
  // from prose. This is the same rule one step earlier, over the row's string values
  // (its keys are identifiers in the bundle, not literals — and `config` as a key is
  // not a sentence a visitor reads).
  const strings = (value, into) => {
    if (typeof value === "string") into.push(value);
    else if (Array.isArray(value)) value.forEach((entry) => strings(entry, into));
    else if (value && typeof value === "object") Object.values(value).forEach((entry) => strings(entry, into));
    return into;
  };
  for (const fact of ENGINE_FACTS.reference.rows) {
    const text = strings(fact.row, []).join("\n");
    for (const tier of [TIER_A, TIER_B]) {
      const hits = scanText(text, fact.config, { tiers: [tier] });
      assert.deepEqual(
        hits.map((hit) => hit.word),
        [],
        `${fact.config}: the shipped Reference row says ${hits.map((hit) => hit.word).join(", ")}`,
      );
    }
  }
});
