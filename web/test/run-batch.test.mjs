// Seam 2: §4 export 6, `solve_batch`, against the real artifact.
//
//   node scripts/build-wasm.mjs && node --test "test/**/*.test.mjs"
//
// A Run is 2,000 games, and the whole point of the export is that they do **not**
// cross the boundary as objects: `out` is a stride-4 `u32` array the *caller*
// allocated through `alloc` (§4.3), and the export returns how many summaries it
// wrote. So the assertions below are about bytes and counts, never JSON:
//
//   * the batch *is* the single-solve path, batched — every word equals what
//     `gen_game` + `solve` produced for that seed (B1/B2), which is the only
//     reason a live Run's Evaluations are comparable with a Reference row's;
//   * an invalid request writes nothing and returns 0, so the pool turns it into a
//     named failure instead of a split full of `status: 1` rows (B4);
//   * the batch touches no §4.1 buffer, so a Run can run beside an interactive
//     solve in the same instance (B5);
//   * seeds wrap rather than saturate, and a fresh view is built after every call
//     because a solve can grow memory and detach the old one (B6/B7).
//
// The engine cannot time itself: `std::time::Instant::now()` traps on
// `wasm32-unknown-unknown`, which is why the summary has four words and no time
// field and why the worker calls this with `count = 1` per game (notes/15 §0.1).

import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { SUMMARY_STRIDE, loadEngineFromBytes } from "../src/core/loader.ts";

const ARTIFACT = fileURLToPath(new URL("../pkg/engine.wasm", import.meta.url));

/** The shipped default configuration's behaviour (the Dataset's `goal8-p40-cap7`). */
const GOAL = 8;
const PIECES = 40;
const CAP = 7;
const BEAM = 25;

let compiled = null;

function artifact() {
  if (!existsSync(ARTIFACT)) {
    throw new Error(`missing ${ARTIFACT} — run \`node scripts/build-wasm.mjs\` first`);
  }
  compiled ??= WebAssembly.compile(new Uint8Array(readFileSync(ARTIFACT)));
  return compiled;
}

/** A fresh instance per test: wasm memory and the §4.1 buffers are per-instance state. */
async function engine() {
  return loadEngineFromBytes(await artifact());
}

/** The single-solve path for one seed: what a batch word-for-word must agree with. */
function single(loaded, seed) {
  const generated = loaded.genGame(seed, PIECES, CAP);
  assert.equal(generated, 0, `gen_game(${seed}) returned ${generated}`);
  const outcome = loaded.solve(GOAL, BEAM);
  assert.equal(outcome.status, 0, `solve(${seed}) returned status ${outcome.status}`);
  return [outcome.won ? 1 : 0, outcome.lines, outcome.evaluations, 0];
}

function words(loaded, out, games) {
  return Array.from(loaded.summaries(out, games));
}

test("§4.3: the summary stride is four u32 words", () => {
  assert.equal(SUMMARY_STRIDE, 4, "won, lines, evaluations, status");
});

test("B1: a chunk writes one summary per seed and returns the count", async () => {
  const loaded = await engine();
  // The single path first, so the batch cannot be its own oracle.
  const expected = [];
  for (let seed = 0; seed < 8; seed += 1) expected.push(single(loaded, seed));

  const out = loaded.allocBatch(8);
  const written = loaded.solveBatch(0, 8, GOAL, PIECES, CAP, BEAM, out);
  assert.equal(written, 8, "eight games asked for, eight written");

  const flat = words(loaded, out, 8);
  assert.equal(flat.length, 32, "4 words x 8 games");
  for (let index = 0; index < 8; index += 1) {
    assert.deepEqual(
      flat.slice(index * 4, index * 4 + 4),
      expected[index],
      `seed ${index}: won, lines, evaluations, status`,
    );
  }
});

test("B2: game i sits at words 4i..4i+3, and a batch of one matches the first game", async () => {
  const loaded = await engine();
  const many = loaded.allocBatch(8);
  assert.equal(loaded.solveBatch(0, 8, GOAL, PIECES, CAP, BEAM, many), 8);
  const first = words(loaded, many, 8).slice(0, 4);

  const one = loaded.allocBatch(1);
  assert.equal(loaded.solveBatch(0, 1, GOAL, PIECES, CAP, BEAM, one), 1);
  assert.deepEqual(words(loaded, one, 1), first, "a batch of one is the first game of a batch of eight");

  // And seed 3 is not seed 0: the stride is an offset, not a repeat.
  const flat = words(loaded, many, 8);
  assert.notDeepEqual(flat.slice(12, 16), first, "game 3 is its own game");
});

test("B3: count = 0 writes nothing and returns 0", async () => {
  const loaded = await engine();
  const out = loaded.allocBatch(4);
  assert.equal(loaded.solveBatch(0, 4, GOAL, PIECES, CAP, BEAM, out), 4);
  const before = words(loaded, out, 4);

  assert.equal(loaded.solveBatch(0, 0, GOAL, PIECES, CAP, BEAM, out), 0, "a chunk on the boundary");
  assert.deepEqual(words(loaded, out, 4), before, "the buffer is untouched");
});

test("B4: an out-of-domain request writes nothing and returns 0", async () => {
  const loaded = await engine();
  const out = loaded.allocBatch(4);
  assert.equal(loaded.solveBatch(0, 4, GOAL, PIECES, CAP, BEAM, out), 4);
  const before = words(loaded, out, 4);

  const bad = [
    ["pieces = 0", [0, 4, GOAL, 0, CAP, BEAM]],
    ["cap = 1", [0, 4, GOAL, PIECES, 1, BEAM]],
    ["beam = 16,385", [0, 4, GOAL, PIECES, CAP, 16_385]],
    ["beam = 0", [0, 4, GOAL, PIECES, CAP, 0]],
    ["goal = 0", [0, 4, 0, PIECES, CAP, BEAM]],
    ["goal = 256", [0, 4, 256, PIECES, CAP, BEAM]],
    ["pieces = 201", [0, 4, GOAL, 201, CAP, BEAM]],
  ];
  for (const [name, args] of bad) {
    assert.equal(loaded.solveBatch(...args, out), 0, `${name}: nothing written`);
    assert.deepEqual(words(loaded, out, 4), before, `${name}: the buffer is untouched`);
  }
});

test("B4b: an out pointer inside the ABI region, or misaligned, writes nothing", async () => {
  const loaded = await engine();
  // 0x00 is the board buffer. Writing a summary there would destroy a live game.
  assert.equal(loaded.solveBatch(0, 1, GOAL, PIECES, CAP, BEAM, 0x00), 0, "0x00 is the board");
  assert.equal(loaded.solveBatch(0, 1, GOAL, PIECES, CAP, BEAM, 0x140), 0, "0x140 is the moves");
  const out = loaded.allocBatch(1);
  assert.equal(loaded.solveBatch(0, 1, GOAL, PIECES, CAP, BEAM, out + 2), 0, "a misaligned out");
  assert.equal(loaded.genGame(0, PIECES, CAP), 0, "the instance still works");
});

test("B5: a batch clobbers no §4.1 buffer, so a Run can run beside an interactive solve", async () => {
  const loaded = await engine();
  assert.equal(loaded.genGame(42, PIECES, CAP), 0);
  const board = Array.from(loaded.board());
  const sequence = loaded.sequence(PIECES);
  const before = loaded.solve(GOAL, BEAM);

  const out = loaded.allocBatch(8);
  assert.equal(loaded.solveBatch(500, 8, GOAL, PIECES, CAP, BEAM, out), 8);

  assert.deepEqual(Array.from(loaded.board()), board, "the board buffer is byte-identical");
  assert.equal(loaded.sequence(PIECES), sequence, "the sequence buffer is byte-identical");
  const after = loaded.solve(GOAL, BEAM);
  assert.deepEqual(after, before, "and the same solve returns the same packed result");
});

test("B6: seeds wrap at u32::MAX rather than saturating on one game", async () => {
  const loaded = await engine();
  const out = loaded.allocBatch(2);
  assert.equal(loaded.solveBatch(0xffff_ffff, 2, GOAL, PIECES, CAP, BEAM, out), 2);

  const last = single(loaded, 0xffff_ffff);
  const zero = single(loaded, 0);
  const flat = words(loaded, out, 2);
  assert.deepEqual(flat.slice(0, 4), last, "0xFFFFFFFF is its own game");
  assert.deepEqual(flat.slice(4, 8), zero, "and the next seed is 0, not 0xFFFFFFFF again");
});

test("B7: a fresh view after every call survives a memory growth", async () => {
  const loaded = await engine();
  // Two hundred single-game batches: enough growth to detach any view taken once.
  let measured = 0;
  for (let seed = 0; seed < 200; seed += 1) {
    const out = loaded.allocBatch(1);
    assert.equal(loaded.solveBatch(seed, 1, GOAL, PIECES, CAP, BEAM, out), 1);
    const summary = loaded.summaries(out, 1);
    assert.equal(summary.length, 4, `seed ${seed}: the view is live`);
    if (summary[3] === 0) measured += 1;
  }
  assert.equal(measured, 200, "every game was measured, none was a detached read of zeros");
});
