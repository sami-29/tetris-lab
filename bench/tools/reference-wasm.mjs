#!/usr/bin/env node
// The wasm half of the Reference run: the numbers a visitor is compared against.
//
//   node bench/tools/reference-wasm.mjs --wasm web/pkg/engine.wasm \
//        --goal 8 --pieces 40 --cap 7 --width 25 --first 0 --last 999 --cal 10
//
// `bench reference` runs this once per configuration and computes the statistics
// itself (`bench/src/reference.rs`), so the p90 has one implementation and the row one
// author. The substrate is the artifact the page loads, loaded through the page's own
// loader — the same file, the same code path, no re-implementation of the ABI. That
// matters because §9.6's ms rows and ms-per-Solve are what a visitor's Run is set
// beside: a native build is 2–4× faster and would be a flattering label.
//
// Protocol, one record per line (any `#` line is a comment):
//
//   sample,<seed>,<won>,<lines>,<evaluations>,<ms>
//   cal_before,<ms>
//   cal_after,<ms>
//   batch_ms,<ms>
//
// The calibration is the bound's normaliser (notes/09 §4.6): `cal` solves of
// seed 0 of goal12-p100-cap7 at width 200, timed the same way, once before and once
// after the batch. `batch_ms` is one clock across the whole seed loop — `genGame`,
// `solve` and the bookkeeping, which is exactly the wall clock a visitor's Run pays.
//
// Two habits that keep the number honest on a shared box, the same ones
// `wasm-timing.mjs` uses:
//
//   * the board is regenerated through `genGame` before *every* timed solve, so every
//     sample solves the game its seed names and not the previous seed's board;
//   * a warm-up pass (two solves, discarded) runs first, so V8's compilation and the
//     engine's first memory growth are not charged to the first sample.
//
// Exit codes: 0 measured, 1 failed, 2 usage.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { loadEngineFromBytes } from "../../web/src/core/loader.ts";

const args = process.argv.slice(2);
const option = (name, fallback) => {
  const index = args.indexOf(`--${name}`);
  if (index === -1) {
    if (fallback === undefined) {
      console.error(`error: --${name} is required`);
      process.exit(2);
    }
    return fallback;
  }
  const value = args[index + 1];
  if (value === undefined) {
    console.error(`error: --${name} needs a value`);
    process.exit(2);
  }
  return value;
};
const number = (name, fallback) => {
  const raw = option(name, fallback);
  const value = Number(raw);
  if (!Number.isFinite(value) || value < 0) {
    console.error(`error: --${name}: ${raw} is not a non-negative number`);
    process.exit(2);
  }
  return value;
};

const ARTIFACT = option("wasm", fileURLToPath(new URL("../../web/pkg/engine.wasm", import.meta.url)));
const goal = number("goal");
const pieces = number("pieces");
const cap = number("cap");
const width = number("width");
const first = number("first", 0);
const last = number("last", 999);
const cal = number("cal", 10);

// The calibration game, fixed: the harder configuration at width 200. Big enough
// that a loaded box's slowdown shows, cheap enough that ten repeats are noise-free.
const CAL = { goal: 12, pieces: 100, cap: 7, width: 200, seed: 0 };

const engine = await loadEngineFromBytes(new Uint8Array(readFileSync(ARTIFACT)));

/** Solve one game at one width, timing the `solve` call and nothing else. */
function timedSolve(seed, solveGoal, solveWidth) {
  const status = engine.genGame(seed, pieces, cap);
  if (status !== 0) {
    throw new Error(`gen_game(${seed}, ${pieces}, ${cap}) returned status ${status}`);
  }
  const started = performance.now();
  const outcome = engine.solve(solveGoal, solveWidth);
  const ms = performance.now() - started;
  if (outcome.status !== 0) {
    throw new Error(`solve(goal ${solveGoal}, width ${solveWidth}) returned status ${outcome.status}`);
  }
  return { won: outcome.won, lines: outcome.lines, evaluations: outcome.evaluations, ms };
}

/** One calibration slice: `cal` timed solves of the fixed game, summed. */
function calibration() {
  if (cal === 0) return 0;
  let total = 0;
  for (let repeat = 0; repeat < cal; repeat += 1) {
    const status = engine.genGame(CAL.seed, CAL.pieces, CAL.cap);
    if (status !== 0) throw new Error(`gen_game(calibration) returned status ${status}`);
    const started = performance.now();
    const outcome = engine.solve(CAL.goal, CAL.width);
    total += performance.now() - started;
    if (outcome.status !== 0) {
      throw new Error(`solve(calibration) returned status ${outcome.status}`);
    }
  }
  return total;
}

console.log(
  `# reference-wasm: abi ${engine.abiVersion}, ${ARTIFACT}, goal ${goal}/${pieces}/cap ${cap}, ` +
    `width ${width}, seeds ${first}-${last}, calibration ${cal} x (goal ${CAL.goal}, width ${CAL.width})`,
);

// Not a sample: two solves that pay for compilation and the first memory growth.
for (let seed = first; seed < first + 2; seed += 1) {
  timedSolve(seed, goal, width);
}

const before = calibration();
console.log(`cal_before,${before.toFixed(3)}`);

const started = performance.now();
for (let seed = first; seed <= last; seed += 1) {
  const sample = timedSolve(seed, goal, width);
  console.log(
    `sample,${seed},${sample.won ? 1 : 0},${sample.lines},${sample.evaluations},${sample.ms.toFixed(3)}`,
  );
}
const batch = performance.now() - started;
console.log(`batch_ms,${batch.toFixed(3)}`);

const after = calibration();
console.log(`cal_after,${after.toFixed(3)}`);
