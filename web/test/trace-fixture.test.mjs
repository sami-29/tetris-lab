// Seam 3: the score band, its per-layer counts and the winning lineage, built
// from a committed trace fixture — **no solver, no wasm, no worker**.
//
// The fixture is produced once from the real engine (`web/scripts/gen-trace-fixture.mjs`
// runs the built artifact) and committed. Two things follow from that, and both are
// the point of this file existing:
//
//   * the band's every figure has a native author: counts, scores, ranks and the
//     lineage are the engine's words, decoded here, never re-derived in JavaScript
//     (a `candidates`/`dedup` counter computed in JS is exactly the fabrication
//     this ticket forbids);
//   * the decoder and the model are production code — the same `decodeTrace` the
//     page runs on a live solve — so a decoder bug cannot pass by being tested
//     against a prettified copy of the data.
//
// The invariants are the ones `engine/tests/trace.rs` and
// `web/test/solve-traced.test.mjs` assert at their own seams; this file is the one
// that can run with no engine at all.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { DEFAULT_BEAM } from "../src/config/width.ts";
import { LINEAGE_END, buildBand, decodeTrace, greedySpine, winningLineage } from "../src/ui/band-model.ts";

const FIXTURE = JSON.parse(
  readFileSync(
    fileURLToPath(new URL("./fixtures/trace-seed0-goal8-p40-cap7-band25.json", import.meta.url)),
    "utf8",
  ),
);

/** The fixture as the page sees it: §4.4's words in, a `Trace` out. */
const TRACE = decodeTrace(FIXTURE.traceWords, FIXTURE.band);

const perLayer = (trace) => trace.perLayerCounts;
const layerOf = (trace, index) => trace.records.slice(trace.layerStart[index], trace.layerStart[index] + trace.perLayerCounts[index]);
const allScores = (trace) => trace.records.map((record) => record.score);

// ── the fixture is what the engine said, and it is still the shipped band ───

test("the fixture was traced at the shipped beam, by this ABI", () => {
  assert.equal(FIXTURE.provenance.abiVersion, 2, "abi_version 2 is the version solve_traced lands at");
  assert.equal(FIXTURE.band, DEFAULT_BEAM, "§2.3: the band is DEFAULT_BEAM states per layer");
  assert.equal(FIXTURE.beamWidth, FIXTURE.band, "and the band is exactly as wide as the beam, no wider");
  assert.ok(FIXTURE.provenance.wasmSha256, "the fixture names the artifact that produced it");
  assert.ok(FIXTURE.game.sequence.length === FIXTURE.game.pieces, "the Sequence rides along");
  // The default game of the page: seed 0, goal 8, 40 pieces, cap 7 (main.ts).
  assert.deepEqual(
    [FIXTURE.game.seed, FIXTURE.game.goal, FIXTURE.game.pieces, FIXTURE.game.heightCap],
    [0, 8, 40, 7],
  );
});

test("the word stream is §4.4's: a count, then five words per record", () => {
  assert.ok(Array.isArray(FIXTURE.traceWords), "the fixture carries the ABI words verbatim, not a prettified graph");
  assert.equal(FIXTURE.traceWords[0], TRACE.records.length, "word 0 is the record count");
  assert.equal(FIXTURE.traceWords.length, 1 + 5 * FIXTURE.traceWords[0], "five i32 per record");
  assert.equal(
    perLayer(TRACE).reduce((sum, count) => sum + count, 0),
    TRACE.records.length,
    "the per-layer counts add up to the record count",
  );
  assert.equal(TRACE.layerCount, perLayer(TRACE).length, "one count per layer");
});

// ── layer order, rank order, parent resolution: the engine's own invariants ─

test("records are in layer order and rank order, and rank 0 is the best score", () => {
  let expectedStart = 0;
  for (let layer = 0; layer < TRACE.layerCount; layer += 1) {
    assert.equal(TRACE.layerStart[layer], expectedStart, `layer ${layer} starts where layer ${layer - 1} ended`);
    assert.equal(perLayer(TRACE)[layer], layerOf(TRACE, layer).length, `layer ${layer}'s count matches its records`);
    const records = layerOf(TRACE, layer);
    assert.ok(records.length > 0, `layer ${layer} holds at least one record`);
    for (const [rank, record] of records.entries()) {
      assert.equal(record.layer, layer, "layer order");
      assert.equal(record.rank, rank, "rank is the record's index inside its layer");
      if (rank > 0) {
        assert.ok(
          record.score <= records[rank - 1].score,
          `layer ${layer} rank ${rank} scores ${record.score} above rank ${rank - 1}'s ${records[rank - 1].score}`,
        );
      }
    }
    expectedStart += records.length;
  }
});

test("every parent_rank resolves in the previous layer", () => {
  for (let layer = 0; layer < TRACE.layerCount; layer += 1) {
    for (const record of layerOf(TRACE, layer)) {
      if (layer === 0) {
        assert.equal(record.parentRank, -1, "§4.4: -1 at layer 0");
        continue;
      }
      assert.notEqual(record.parentRank, -2, "the fixture is not clipped: band === beam");
      assert.ok(
        record.parentRank >= 0 && record.parentRank < perLayer(TRACE)[layer - 1],
        `layer ${layer} rank ${record.rank} names parent ${record.parentRank}, layer ${layer - 1} holds ${perLayer(TRACE)[layer - 1]}`,
      );
      assert.equal(
        layerOf(TRACE, layer - 1)[record.parentRank].rank,
        record.parentRank,
        "the parent's own record names the same rank",
      );
    }
  }
});

test("lines are cumulative along every parent edge", () => {
  for (let layer = 1; layer < TRACE.layerCount; layer += 1) {
    for (const record of layerOf(TRACE, layer)) {
      const parent = layerOf(TRACE, layer - 1)[record.parentRank];
      assert.ok(
        record.lines >= parent.lines,
        `layer ${layer} rank ${record.rank} has ${record.lines} lines, its parent ${parent.lines}`,
      );
    }
  }
});

test("the band bound is respected: no layer keeps more records than the band", () => {
  for (let layer = 0; layer < TRACE.layerCount; layer += 1) {
    assert.ok(
      perLayer(TRACE)[layer] <= FIXTURE.band,
      `layer ${layer} keeps ${perLayer(TRACE)[layer]} of a ${FIXTURE.band}-record band`,
    );
  }
  assert.ok(
    TRACE.records.length <= FIXTURE.band * TRACE.layerCount,
    "the record count fits the buffer the band sizes",
  );
});

// ── the goldens: the numbers the band prints come from these ────────────────

test("the per-layer counts are the fixture's, and they are the band's own numbers", () => {
  assert.deepEqual(perLayer(TRACE), FIXTURE.expect.perLayerCounts, "per-layer counts moved");
  const model = buildBand(TRACE);
  assert.equal(model.rows.length, TRACE.layerCount, "one row per Layer");
  for (const [index, row] of model.rows.entries()) {
    assert.equal(row.layer, index, "rows are in Layer order");
    assert.equal(row.count, FIXTURE.expect.perLayerCounts[index], `row ${index} prints the engine's count`);
  }
  assert.equal(model.records, TRACE.records.length, "the model's record count is the trace's");
});

// ── the winning lineage ───────────────────────────────────────────────────

test("the winning lineage runs from the best final state back to the root, one point per layer", () => {
  const last = layerOf(TRACE, TRACE.layerCount - 1);
  const lineage = winningLineage(TRACE);
  assert.equal(lineage.length, TRACE.layerCount, "the lineage is drawn on every row");
  assert.equal(lineage[0].layer, 0, "it starts at the root");
  assert.equal(lineage[0].rank, -1, `the root has no parent: ${LINEAGE_END}`);
  assert.equal(lineage[TRACE.layerCount - 1].rank, last[0].rank, "it ends at the first record of the last layer, i.e. the best final score");
  assert.equal(lineage[TRACE.layerCount - 1].score, last[0].score);
  for (let layer = 1; layer < lineage.length; layer += 1) {
    assert.equal(
      lineage[layer].parentRank,
      lineage[layer - 1].rank,
      `layer ${layer}'s lineage point hangs off layer ${layer - 1}'s`,
    );
  }
  // Each point's score is the record's own score, so its place on the axis is
  // the record's score and never the row's best.
  for (const point of lineage) {
    const record = layerOf(TRACE, point.layer).find((entry) => entry.rank === point.rank);
    assert.equal(point.score, record.score, `layer ${point.layer} lineage score`);
  }
  assert.deepEqual(
    lineage.map((point) => point.rank),
    FIXTURE.expect.lineageRanks,
    "the lineage's rank path moved",
  );
  assert.deepEqual(
    lineage.map((point) => point.score),
    FIXTURE.expect.lineageScores,
    "the lineage's score path moved",
  );
});

test("the lineage is not the leader: its rank wanders, so following the leader is wrong", () => {
  const lineage = winningLineage(TRACE);
  const offFront = lineage.filter((point) => point.layer > 0 && point.rank > 0).length;
  assert.ok(
    offFront >= Math.ceil((TRACE.layerCount - 1) / 2),
    `the lineage is off the row's front in ${offFront} of ${TRACE.layerCount - 1} non-root layers`,
  );
  const deepest = Math.max(...lineage.map((point) => point.rank));
  assert.equal(deepest, FIXTURE.expect.deepestLineageRank, "the deepest rank the lineage reaches");
  assert.ok(deepest > 0, "a follow-the-leader affordance would never find this path");

  // The greedy descent — always the best child — is a different path, and it is
  // the one the picture must not draw.
  const spine = greedySpine(TRACE);
  assert.equal(spine.length, FIXTURE.expect.greedySpineLength, "the greedy descent's length");
  assert.ok(
    spine.length < TRACE.layerCount,
    "a greedy descent dead-ends before the last Layer",
  );
  assert.notDeepEqual(
    spine.map((point) => point.rank),
    lineage.map((point) => point.rank),
    "the greedy descent is not the lineage",
  );
});

test("the lineage is drawn outside a bound that excludes it, at its own score", () => {
  // The band is as wide as the beam, so the lineage can never leave the record
  // set; what it does leave is any *narrower* bound a drill-down would set (the
  // tree's top-N). The drawing must place it at its own score and never clamp it
  // to the bound's edge or to the row's leader.
  const bound = Math.max(1, Math.floor(FIXTURE.band / 2));
  const model = buildBand(TRACE, { bound });
  const outside = model.lineage.filter((point) => point.rank >= bound);
  assert.ok(outside.length > 0, `the fixture's lineage leaves a ${bound}-record bound, or this assertion is vacuous`);
  assert.equal(model.outsideBound, outside.length, "every point beyond the bound is counted");
  for (const point of outside) {
    const row = model.rows[point.layer];
    const expected = row.marks.find((mark) => mark.score === point.score);
    assert.ok(expected, `layer ${point.layer}: the mark at the lineage's score exists`);
    assert.equal(point.u, expected.u, "the lineage is drawn at its score, not at the bound's edge");
    assert.ok(point.outsideBound, "and it is flagged as beyond the bound");
  }
  // The default bound is the band's own: nothing is outside it.
  assert.equal(buildBand(TRACE).outsideBound, 0, "at band === beam no record leaves the band");
});

// ── the axis: placed by score, never by parent or by rank ──────────────────

test("one row per Layer, states placed by score on one shared axis", () => {
  const model = buildBand(TRACE);
  const scores = allScores(TRACE);
  assert.equal(model.axis.min, Math.min(...scores), "the axis floor is the worst score in the trace");
  assert.equal(model.axis.max, Math.max(...scores), "the axis ceiling is the best score in the trace");
  assert.ok(model.axis.ticks.length >= 2, "the axis is labelled");

  for (const row of model.rows) {
    const records = layerOf(TRACE, row.layer);
    assert.equal(row.best, records[0].score, "the row's best is its rank 0");
    assert.equal(row.worst, records[records.length - 1].score, "the row's worst is its last rank");
    for (const mark of row.marks) {
      const expected = (model.axis.max - mark.score) / (model.axis.max - model.axis.min);
      assert.equal(mark.u, expected, `layer ${row.layer}: a mark's place is its score's place on the axis`);
      assert.ok(mark.u >= 0 && mark.u <= 1, "inside the plot");
      const shared = records.filter((record) => record.score === mark.score).length;
      assert.equal(mark.count, shared, `layer ${row.layer}: ${mark.count} states share score ${mark.score}`);
    }
    // Marks are in score order across the row, which is the only ordering the
    // picture uses.
    for (let index = 1; index < row.marks.length; index += 1) {
      assert.ok(row.marks[index - 1].score > row.marks[index].score, "marks are score-ordered");
      assert.ok(row.marks[index - 1].u < row.marks[index].u, "and placed left to right, best first");
    }
  }
});

test("placement is a function of score, not of rank: reordering a layer changes nothing", () => {
  const model = buildBand(TRACE);
  // Reverse every layer's records (rank order destroyed, scores untouched) and
  // rebuild: the marks must be identical, because they are placed by score.
  const reversed = [];
  for (let layer = 0; layer < TRACE.layerCount; layer += 1) {
    reversed.push(...layerOf(TRACE, layer).slice().reverse());
  }
  const shuffled = decodeTrace(
    [reversed.length, ...reversed.flatMap((record) => [record.layer, record.rank, record.score, record.lines, record.parentRank])],
    FIXTURE.band,
  );
  const rebuilt = buildBand(shuffled);
  for (const [index, row] of rebuilt.rows.entries()) {
    assert.deepEqual(
      row.marks.map((mark) => [mark.score, mark.count, mark.u]),
      model.rows[index].marks.map((mark) => [mark.score, mark.count, mark.u]),
      `layer ${index}: the same scores must land in the same places`,
    );
  }
});

// ── the labels are the engine's numbers, in words ─────────────────────────

test("the band's caption names the band, the layers and the records it drew", () => {
  const model = buildBand(TRACE);
  const caption = model.caption;
  assert.match(caption, /band 25/, "the caption states the band it was traced at");
  assert.match(caption, new RegExp(`${TRACE.layerCount} layers`), "and the layer count");
  assert.match(caption, new RegExp(`${TRACE.records.length} records`), "and the record count");
  assert.match(caption, /beam/, "and that the band is the beam");
  assert.equal(model.label, `traced at band ${FIXTURE.band} = the shipped beam`, "the one line the inspector prints");
});

test("the band model and the decoder are pure: no wasm, no worker, no solver", () => {
  const source = (name) =>
    readFileSync(fileURLToPath(new URL(`../src/ui/${name}`, import.meta.url)), "utf8");
  for (const name of ["trace.ts", "band-model.ts"]) {
    const text = source(name);
    assert.ok(!/WebAssembly/.test(text), `${name} must not touch WebAssembly`);
    assert.ok(!/core\/loader|core\/protocol|workers\//.test(text), `${name} must not reach the worker or the loader`);
    assert.ok(!/\bworker\b/i.test(text), `${name} must not mention a worker`);
  }
});
