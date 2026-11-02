// Seam 2, the traced solve: the real wasm artifact through the real loader.
//
// `engine/tests/trace.rs` proves the engine's trace invariants natively and
// `web/test/trace-fixture.test.mjs` proves the band's from a frozen fixture. This
// file is the one seam that can see §4.4's *word* layout: a buffer obtained from
// `alloc`, a count at word 0, five `i32` per record, and an export whose packed
// result must be bit-identical to `solve`'s.
//
// Three faults are only visible here:
//
//   * `solve_traced` and `solve` drifting apart (the spec's "returns the same
//     packed i64 as solve" is a claim about two exports, not about one);
//   * the trace region not being the `alloc`'d region the loader reads, or an
//     `i32` view landing on an unaligned offset;
//   * an overrun — a game with 200 pieces recording more records than the buffer
//     the band sized, which would corrupt the wasm heap rather than fail a test.
//
// The last test is the behaviour binding for the committed fixture: the words the
// browser produces for the default game must still be the words in
// `web/test/fixtures/trace-seed0-goal8-p40-cap7-band25.json`. An engine change
// fails here and in `cargo test`, so the fixture cannot rot silently.

import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { ABI_END, loadEngineFromBytes } from "../src/core/loader.ts";
import { DEFAULT_BEAM } from "../src/config/width.ts";
import { decodeTrace } from "../src/ui/band-model.ts";

const ARTIFACT = fileURLToPath(new URL("../pkg/engine.wasm", import.meta.url));
const FIXTURE = JSON.parse(
  readFileSync(
    fileURLToPath(new URL("./fixtures/trace-seed0-goal8-p40-cap7-band25.json", import.meta.url)),
    "utf8",
  ),
);

/** The page's default game, and the fixture's. */
const DEFAULT_GAME = { seed: 0, goal: 8, pieces: 40, heightCap: 7 };

function wasmBytes() {
  if (!existsSync(ARTIFACT)) {
    throw new Error(`missing ${ARTIFACT} — run \`node scripts/build-wasm.mjs\` first`);
  }
  return new Uint8Array(readFileSync(ARTIFACT));
}

async function boot() {
  const engine = await loadEngineFromBytes(wasmBytes());
  return engine;
}

function generate(engine, game = DEFAULT_GAME) {
  const status = engine.genGame(game.seed, game.pieces, game.heightCap);
  assert.equal(status, 0, `gen_game(${game.seed}, ${game.pieces}, ${game.heightCap})`);
}

const layerStarts = (trace) => trace.layerStart;
const layerOf = (trace, index) =>
  trace.records.slice(layerStarts(trace)[index], layerStarts(trace)[index] + trace.perLayerCounts[index]);

// ── the export ────────────────────────────────────────────────────────────

test("the loader exposes solve_traced at abi_version 2", async () => {
  const engine = await boot();
  assert.equal(engine.abiVersion, 2);
  assert.equal(typeof engine.solveTraced, "function", "the loader binds solve_traced");
  assert.equal(typeof engine.traceWords, "function", "and can read the trace region");
  assert.equal(
    engine.traceBytes,
    DEFAULT_BEAM * 200 * 20,
    "§4.4 sizes the region as band x pieces x 20 bytes, band at DEFAULT_BEAM and pieces at the ABI maximum",
  );
  assert.ok(engine.scratchOffset >= ABI_END, "the trace region sits above the §4 buffers");
  assert.equal(engine.scratchOffset % 4, 0, "an Int32Array view needs a 4-byte-aligned offset");
});

test("solve_traced returns the same packed result as solve", async () => {
  const engine = await boot();
  for (const [goal, beam] of [[8, 25], [8, 1], [12, 8]]) {
    generate(engine);
    const plain = engine.solve(goal, beam);
    const plainMoves = [...engine.moves(plain.moveCount)];
    generate(engine);
    const traced = engine.solveTraced(goal, beam, DEFAULT_BEAM);
    const tracedMoves = [...engine.moves(traced.moveCount)];
    assert.deepEqual(traced, plain, `goal ${goal} beam ${beam}: the packed result moved`);
    assert.deepEqual(tracedMoves, plainMoves, `goal ${goal} beam ${beam}: the Move list moved`);
    assert.ok(engine.traceWords().length > 1, `goal ${goal} beam ${beam}: nothing was traced`);
  }
});

// ── the words ─────────────────────────────────────────────────────────────

test("the trace region is §4.4's: a count word, then five i32 per record", async () => {
  const engine = await boot();
  generate(engine);
  engine.solveTraced(DEFAULT_GAME.goal, DEFAULT_BEAM, DEFAULT_BEAM);

  const words = engine.traceWords();
  const n = words[0];
  assert.ok(n > 0, "a solved game is traced");
  assert.equal(words.length, 1 + 5 * n, "five words per record and no more");
  assert.ok(4 + 20 * n <= engine.traceBytes, "the trace fits the region the band sized");

  const trace = decodeTrace(words, DEFAULT_BEAM);
  assert.equal(trace.records.length, n);
  assert.equal(trace.records[0].layer, 0, "layer order starts at the root");
  assert.equal(trace.records[0].parentRank, -1, "§4.4: -1 at layer 0");
  assert.equal(layerOf(trace, 0).length, 1, "layer 0 is the root board alone");
  for (let layer = 0; layer < trace.layerCount; layer += 1) {
    const records = layerOf(trace, layer);
    assert.ok(records.length <= DEFAULT_BEAM, `layer ${layer} keeps ${records.length} of a ${DEFAULT_BEAM}-record band`);
    for (const [rank, record] of records.entries()) {
      assert.equal(record.layer, layer);
      assert.equal(record.rank, rank);
      if (rank > 0) assert.ok(record.score <= records[rank - 1].score, `layer ${layer} scores descend`);
      if (layer > 0) {
        assert.notEqual(record.parentRank, -2, "the band equals the beam here, so nothing is unrecorded");
        assert.ok(
          record.parentRank >= 0 && record.parentRank < layerOf(trace, layer - 1).length,
          `layer ${layer} rank ${record.rank} names parent ${record.parentRank}`,
        );
      }
    }
  }
});

test("a second solve_traced rewrites the count word and the records it wrote", async () => {
  const engine = await boot();
  generate(engine);
  engine.solveTraced(DEFAULT_GAME.goal, DEFAULT_BEAM, DEFAULT_BEAM);
  const wide = engine.traceWords();
  const wideCount = wide[0];

  // A narrower band, the same game: fewer records per layer, same game.
  engine.solveTraced(DEFAULT_GAME.goal, DEFAULT_BEAM, 4);
  const narrow = engine.traceWords();
  assert.notEqual(narrow[0], wideCount, "the count word is rewritten");
  assert.ok(narrow[0] < wideCount, `a band of 4 records fewer: ${narrow[0]} vs ${wideCount}`);
  const trace = decodeTrace(narrow, 4);
  for (let layer = 0; layer < trace.layerCount; layer += 1) {
    assert.ok(layerOf(trace, layer).length <= 4, `layer ${layer} after the narrow band`);
  }

  // And back again: nothing of the narrow trace survives past the count.
  const result = engine.solveTraced(DEFAULT_GAME.goal, DEFAULT_BEAM, DEFAULT_BEAM);
  assert.equal(result.status, 0);
  assert.deepEqual([...engine.traceWords()], [...wide], "the same call reproduces the same words");
});

test("a band narrower than the beam marks the parents it did not keep", async () => {
  const engine = await boot();
  generate(engine);
  engine.solveTraced(DEFAULT_GAME.goal, DEFAULT_BEAM, 4);
  const trace = decodeTrace(engine.traceWords(), 4);
  let unrecorded = 0;
  for (let layer = 1; layer < trace.layerCount; layer += 1) {
    const previous = layerOf(trace, layer - 1).length;
    for (const record of layerOf(trace, layer)) {
      if (record.parentRank === -2) {
        unrecorded += 1;
        assert.equal(previous, 4, "a -2 parent is only honest when the layer above was clipped");
        continue;
      }
      assert.ok(
        record.parentRank >= 0 && record.parentRank < previous,
        `layer ${layer} rank ${record.rank} names parent ${record.parentRank} of ${previous}`,
      );
    }
  }
  assert.ok(unrecorded > 0, "the clipped trace really did lose parents");
});

test("a 200-piece game cannot overrun the region the band sized", async () => {
  const engine = await boot();
  generate(engine, { seed: 0, goal: 8, pieces: 200, heightCap: 7 });
  const result = engine.solveTraced(8, DEFAULT_BEAM, DEFAULT_BEAM);
  assert.equal(result.status, 0);
  const words = engine.traceWords();
  const n = words[0];
  assert.ok(n > 0, "the long game is traced");
  assert.ok(
    4 + 20 * n <= engine.traceBytes,
    `${n} records need ${4 + 20 * n} bytes, the region is ${engine.traceBytes}`,
  );
  const trace = decodeTrace(words, DEFAULT_BEAM);
  assert.equal(trace.layerStart.length, trace.layerCount);
  assert.ok(trace.layerCount <= 200 + 1, `a 200-piece game traces at most 201 layers, got ${trace.layerCount}`);
});

test("solving before a game is generated is status 4 and an empty trace", async () => {
  const engine = await boot();
  const result = engine.solveTraced(8, DEFAULT_BEAM, DEFAULT_BEAM);
  assert.equal(result.status, 4, "illegal ABI state, not a trap");
  assert.equal(engine.traceWords()[0], 0, "and the word 0 says the trace is empty");
});

// ── the behaviour binding for the committed fixture ───────────────────────

test("the committed fixture is the trace this artifact produces", async () => {
  const engine = await boot();
  generate(engine);
  const result = engine.solveTraced(DEFAULT_GAME.goal, DEFAULT_BEAM, FIXTURE.band);
  assert.equal(result.status, 0);
  assert.equal(
    String(result.evaluations),
    String(FIXTURE.solve.evaluations),
    "the fixture's evaluation count is still this engine's",
  );
  assert.equal(result.won, FIXTURE.solve.won);
  assert.equal(result.lines, FIXTURE.solve.lines);
  assert.equal(result.moveCount, FIXTURE.solve.moveCount);
  assert.deepEqual(
    [...engine.traceWords()],
    FIXTURE.traceWords,
    "the fixture is stale: re-freeze it with `node web/scripts/gen-trace-fixture.mjs`",
  );
  assert.equal(engine.sequence(FIXTURE.game.pieces), FIXTURE.game.sequence, "and it is the same game");
  assert.deepEqual(
    [...engine.board()].map((word) => `0x${word.toString(16)}`),
    FIXTURE.game.board,
    "and the same starting Board",
  );
});
