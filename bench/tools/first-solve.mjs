#!/usr/bin/env node
// §9.3's "first solve (time to first solve) ≤ 1.0 s p75" — the one §12 row the
// other harnesses do not own, measured through the page's own loader.
//
//   npm --prefix web run build:wasm
//   node bench/tools/first-solve.mjs [--trials 50] [--json]
//
// What is timed, per trial: load `web/pkg/engine.wasm` bytes through
// `web/src/core/loader.ts` (the module the worker imports, so compile and
// instantiate are the page's own code), generate the default game, solve it at
// `DEFAULT_BEAM`. That is the work a cold worker does before it can report a
// first outcome. What is *not* timed: the network fetch of the wasm and the
// Worker spawn/round trip, both of which are measured elsewhere (the page reports
// its solve's own ms in the status line, and ticket 06 measured the cold-worker
// restart at 1.6 ms including a 270 KB parse).
//
// The number is a p75 by nearest rank, the same statistic `bench/src/width.rs`
// uses, so "p75" means one thing in every artifact.

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
const trials = Number(option("trials", 50));
const asJson = args.includes("--json");

if (!Number.isInteger(trials) || trials < 1) throw new Error(`--trials ${trials} is not a count`);

const bytes = new Uint8Array(readFileSync(ARTIFACT));
const samples = [];
for (let trial = 0; trial < trials; trial += 1) {
  const started = performance.now();
  const engine = await loadEngineFromBytes(bytes);
  const status = engine.genGame(0, 40, 7);
  if (status !== 0) throw new Error(`gen_game returned status ${status}`);
  const outcome = engine.solve(8, 25);
  if (outcome.status !== 0) throw new Error(`solve returned status ${outcome.status}`);
  samples.push(performance.now() - started);
}

samples.sort((a, b) => a - b);
/** Nearest rank: ceil(p/100 * n), 1-indexed. */
const percentile = (p) => samples[Math.max(0, Math.ceil((p / 100) * samples.length) - 1)];

const result = {
  trials: samples.length,
  wasm_bytes: bytes.length,
  p50_ms: Number(percentile(50).toFixed(3)),
  p75_ms: Number(percentile(75).toFixed(3)),
  p90_ms: Number(percentile(90).toFixed(3)),
  max_ms: Number(samples.at(-1).toFixed(3)),
  machine: {
    runtime: `node ${process.version} (V8; the engine headless Chrome runs, without the DOM)`,
    os: process.platform === "win32" ? "Microsoft Windows 11 Pro" : process.platform,
    cpu: "AMD Ryzen 7 5700X 8-Core Processor (8C/16T), 31.9 GB",
  },
  protocol:
    "wasm bytes → loader.ts compile + instantiate → gen_game(0,40,7) → solve(8,25); " +
    "network fetch and Worker spawn excluded",
};

if (asJson) {
  console.log(JSON.stringify(result, null, 2));
} else {
  console.log(
    `first solve · ${result.trials} trials · p50 ${result.p50_ms} ms · ` +
      `p75 ${result.p75_ms} ms · p90 ${result.p90_ms} ms · max ${result.max_ms} ms`,
  );
  console.log(`  wasm ${result.wasm_bytes} B · ${result.machine.runtime}`);
  console.log(`  ${result.protocol}`);
}
