#!/usr/bin/env node
// The wasm half of the width curve: how long does a Solve actually take in the
// artifact the page loads?
//
//   node bench/tools/wasm-timing.mjs --goal 8 --pieces 40 --cap 7 \
//        --width 400 --trials 3 --seeds 200
//
// §9.3's budgets are *browser* budgets, so the Width cap has to be named on wasm,
// not on a native build that is 2–4× faster. This script loads
// `web/pkg/engine.wasm` through the page's own loader — the same file the browser
// gets, the same code path as the page, no re-implementation of the ABI — and
// prints one CSV row per solve:
//
//   trial,index,won,evaluations,ms
//
// `bench width-curve` runs it once per (bed, width) and computes the p90 itself,
// so the statistic has exactly one implementation (bench/src/width.rs).
//
// Two habits that keep the number honest on a shared box:
//
//   * the board is regenerated through `gen_game` before every timed solve, so
//     every sample solves the game its seed names, not the previous seed's board;
//   * a warm-up pass (two solves at width 25, discarded) runs first, so V8's
//     compilation and the engine's first memory growth are not charged to the
//     first sample. The warm-up rows are commented out with `#`.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { loadEngineFromBytes } from "../../web/src/core/loader.ts";

const ARTIFACT = fileURLToPath(new URL("../../web/pkg/engine.wasm", import.meta.url));

const args = process.argv.slice(2);
const option = (name, fallback) => {
  const index = args.indexOf(`--${name}`);
  if (index === -1) return fallback;
  const value = args[index + 1];
  if (value === undefined) throw new Error(`--${name} needs a value`);
  return value;
};
const number = (name, fallback) => {
  const raw = option(name, undefined);
  if (raw === undefined) {
    if (fallback === undefined) throw new Error(`--${name} is required`);
    return fallback;
  }
  const value = Number(raw);
  if (!Number.isFinite(value) || value < 0) throw new Error(`--${name}: ${raw} is not a number`);
  return value;
};

const goal = number("goal");
const pieces = number("pieces");
const cap = number("cap");
const width = number("width");
const trials = number("trials", 3);
const seeds = number("seeds", 200);
const firstSeed = number("first-seed", 0);

const engine = await loadEngineFromBytes(new Uint8Array(readFileSync(ARTIFACT)));

/** Solve one game at one width, timing the `solve` call and nothing else. */
function timedSolve(seed, beamWidth) {
  const status = engine.genGame(seed, pieces, cap);
  if (status !== 0) throw new Error(`gen_game(${seed}, ${pieces}, ${cap}) returned status ${status}`);
  const started = performance.now();
  const outcome = engine.solve(goal, beamWidth);
  const ms = performance.now() - started;
  if (outcome.status !== 0) {
    throw new Error(`solve(goal ${goal}, width ${beamWidth}) returned status ${outcome.status}`);
  }
  return { won: outcome.won, evaluations: outcome.evaluations, ms };
}

// The warm-up is not a sample: it pays for compilation, the first memory growth
// and the first board of every shape. Two solves at the narrowest width.
console.log(`# warm-up: abi ${engine.abiVersion}, ${ARTIFACT}`);
for (let seed = firstSeed; seed < firstSeed + 2; seed += 1) {
  timedSolve(seed, 25);
}

for (let trial = 0; trial < trials; trial += 1) {
  for (let index = 0; index < seeds; index += 1) {
    const seed = firstSeed + index;
    const sample = timedSolve(seed, width);
    console.log(
      `${trial},${index},${sample.won ? 1 : 0},${sample.evaluations},${sample.ms.toFixed(3)}`,
    );
  }
}
