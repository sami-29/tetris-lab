// Freeze the trace fixture: run the **real** engine (the built wasm, through the
// real loader) once and write what it said into `test/fixtures/`.
//
//   node scripts/gen-trace-fixture.mjs
//
// It is committed output, generated once, never hand-edited. Two consumers:
//
//   * `web/test/trace-fixture.test.mjs` — the band, its per-layer counts and the
//     winning lineage from the fixture alone: no solver, no worker, no wasm. That
//     is build-spec Seam 3.
//   * `web/test/solve-traced.test.mjs` — the same words, asserted against a live
//     `solve_traced` call, so a stale fixture fails a test instead of quietly
//     shipping a picture of a search the engine no longer runs.
//
// The words are stored **verbatim** (§4.4's `[n, ...5n i32]`), not as a prettified
// per-layer object graph: the test then runs the same `decodeTrace` the page runs,
// so a decoder bug cannot pass. `expect` is written from the same decode, which is
// why this file — not the test — is the author of the goldens.

import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

import { loadEngineFromBytes } from "../src/core/loader.ts";
import { DEFAULT_BEAM } from "../src/config/width.ts";
import { buildBand, greedySpine, winningLineage } from "../src/ui/band-model.ts";
import { decodeTrace } from "../src/ui/trace.ts";

const ARTIFACT = fileURLToPath(new URL("../pkg/engine.wasm", import.meta.url));
const OUT = fileURLToPath(
  new URL("../test/fixtures/trace-seed0-goal8-p40-cap7-band25.json", import.meta.url),
);

/** The page's default game (`main.ts`'s `DEFAULTS`), at the shipped band. */
const GAME = { seed: 0, goal: 8, pieces: 40, heightCap: 7 };
const BAND = DEFAULT_BEAM;

const bytes = new Uint8Array(readFileSync(ARTIFACT));
const engine = await loadEngineFromBytes(bytes);

const generated = engine.genGame(GAME.seed, GAME.pieces, GAME.heightCap);
if (generated !== 0) throw new Error(`gen_game returned ${generated}`);
const board = [...engine.board()].map((word) => `0x${word.toString(16)}`);
const sequence = engine.sequence(GAME.pieces);

const plain = engine.solve(GAME.goal, BAND);
const traced = engine.solveTraced(GAME.goal, BAND, BAND);
if (plain.status !== 0 || traced.status !== 0) throw new Error("the default game must solve");
if (
  plain.won !== traced.won ||
  plain.lines !== traced.lines ||
  plain.evaluations !== traced.evaluations ||
  plain.moveCount !== traced.moveCount
) {
  throw new Error("solve_traced disagrees with solve: that is a spec violation, not a fixture");
}

const traceWords = [...engine.traceWords()];
const trace = decodeTrace(traceWords, BAND);
const model = buildBand(trace, { sequence, shippedBeam: DEFAULT_BEAM });
const lineage = winningLineage(trace);
const spine = greedySpine(trace);

const fixture = {
  fixture: "trace-seed0-goal8-p40-cap7-band25",
  provenance: {
    engineCommit: process.env.GIT_COMMIT ?? "uncommitted",
    wasmSha256: createHash("sha256").update(bytes).digest("hex"),
    wasmBytes: bytes.length,
    abiVersion: engine.abiVersion,
    evaluator: "dellacherie-thiery",
    band: BAND,
    beamWidth: BAND,
    defaultBeamAtExport: DEFAULT_BEAM,
    generatedBy: "node web/scripts/gen-trace-fixture.mjs (solve_traced through web/src/core/loader.ts)",
    fixtureVersion: 1,
  },
  game: { ...GAME, board, sequence },
  band: BAND,
  beamWidth: BAND,
  // The packed i64's fields, as the loader unpacks them. The packed word itself is
  // not stored: it is `solve_traced`'s return value, and the test compares the
  // fields it can read on both sides (`solve` and `solve_traced`) rather than a
  // decimal a JavaScript reader would have to trust.
  solve: {
    won: traced.won,
    lines: traced.lines,
    evaluations: traced.evaluations,
    status: traced.status,
    moveCount: traced.moveCount,
  },
  traceWords,
  expect: {
    records: trace.records.length,
    layers: trace.layerCount,
    perLayerCounts: [...trace.perLayerCounts],
    lineageRanks: lineage.map((point) => point.rank),
    lineageScores: lineage.map((point) => point.score),
    deepestLineageRank: Math.max(...lineage.map((point) => point.rank)),
    greedySpineLength: spine.length,
    greedySpineRanks: spine.map((point) => point.rank),
    rootScore: trace.records[0].score,
    axis: model.axis,
  },
};

writeFileSync(OUT, `${JSON.stringify(fixture, null, 2)}\n`);
console.log(
  `wrote ${OUT}\n  ${trace.layerCount} layers, ${trace.records.length} records, ` +
    `${traceWords.length} words, lineage depth ${Math.max(...lineage.map((point) => point.rank))}, ` +
    `greedy spine ${spine.length} of ${trace.layerCount} layers, packed ${plain.won}/${plain.lines}`,
);
