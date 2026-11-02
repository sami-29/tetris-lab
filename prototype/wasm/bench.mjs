#!/usr/bin/env node
// Node harness for the ticket 06 wasm artifacts: per-game timings, the batch
// path, SIMD parity, and the cost of standing up a fresh instance after a
// worker is terminated.
//
//   node prototype/wasm/bench.mjs --wasm prototype/artifacts/wasm-u32.wasm \
//     --games prototype/data/games_goal8_p40_seed0-999.txt --goal 8 \
//     --out prototype/data/wasm-u32.jsonl --check prototype/data/py-asis.jsonl

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";

const args = (() => {
  const out = { goal: 8, reps: 3, limit: 0, timeoutMs: 0 };
  const argv = process.argv.slice(2);
  for (let i = 0; i < argv.length; i += 1) {
    const key = argv[i];
    const val = argv[i + 1];
    switch (key) {
      case "--wasm": out.wasm = val; i += 1; break;
      case "--games": out.games = val; i += 1; break;
      case "--goal": out.goal = Number(val); i += 1; break;
      case "--reps": out.reps = Number(val); i += 1; break;
      case "--limit": out.limit = Number(val); i += 1; break;
      case "--out": out.out = val; i += 1; break;
      case "--check": out.check = val; i += 1; break;
      default: throw new Error(`unknown flag ${key}`);
    }
  }
  if (!out.wasm || !out.games) throw new Error("--wasm and --games are required");
  return out;
})();

const nowMs = () => Number(process.hrtime.bigint()) / 1e6;

function quantile(sorted, p) {
  return sorted[Math.min(sorted.length - 1, Math.round(p * (sorted.length - 1)))];
}

const wasmBytes = readFileSync(args.wasm);
const gamesText = readFileSync(args.games, "utf8");

// ── instantiate ───────────────────────────────────────────────────────────────

const tCompile = nowMs();
const module = await WebAssembly.compile(wasmBytes);
const compileMs = nowMs() - tCompile;

const tInst = nowMs();
const instance = await WebAssembly.instantiate(module);
const firstInstantiateMs = nowMs() - tInst;

const ex = instance.exports;
const marker = ex.variant_marker();
const variant = [
  marker & 1 ? "u32" : "",
  marker & 2 ? "u64" : "",
  marker & 4 ? "+simd" : "",
  marker & 8 ? "+lines_live" : "",
].filter(Boolean).join("");

function loadGames(text, inst = ex) {
  const bytes = new TextEncoder().encode(text);
  const ptr = inst.alloc(bytes.length);
  new Uint8Array(inst.memory.buffer).set(bytes, ptr);
  const n = inst.load_games(ptr, bytes.length);
  inst.dealloc(ptr, bytes.length);
  return n;
}

const total = loadGames(gamesText);
const games = args.limit > 0 ? Math.min(args.limit, total) : total;

// ── SIMD parity gate ──────────────────────────────────────────────────────────

let parityBad = null;
let parityBoards = 0;
if (marker & 4) {
  // Boards from the games file plus adversarial random ones: full columns,
  // single bits, empty boards and hole patterns.
  const boards = [];
  const pushBoard = (words) => boards.push(...words);
  const lines = gamesText.split("\n");
  for (let i = 0; i < lines.length; i += 1) {
    if (!lines[i].startsWith("seed ")) continue;
    const cols = Array(10).fill(0);
    for (let r = 0; r < 20; r += 1) {
      const row = lines[i + 1 + r];
      for (let c = 0; c < 10; c += 1) {
        if (row[c] === "1") cols[c] |= 1 << (19 - r);
      }
    }
    pushBoard(cols);
  }
  let state = 0x12345678;
  const rnd = () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    return state;
  };
  for (let i = 0; i < 40000; i += 1) {
    const cols = Array(10);
    for (let c = 0; c < 10; c += 1) {
      const h = rnd() % 21;
      const bits = h === 0 ? 0 : rnd() >>> (32 - h);
      // Mix in holes: clear a random set bit below the top.
      cols[c] = (bits & ((1 << h) - 1)) >>> 0;
      if (h > 2 && rnd() % 3 === 0) cols[c] &= ~(1 << (rnd() % h));
    }
    pushBoard(cols);
  }
  pushBoard(Array(10).fill(0xfffff));
  pushBoard(Array(10).fill(0));

  parityBoards = boards.length / 10;
  const ptr = ex.alloc(boards.length * 4);
  new Uint32Array(ex.memory.buffer, ptr, boards.length).set(boards);
  parityBad = ex.parity_check(ptr, parityBoards);
  ex.dealloc(ptr, boards.length * 4);
}

// ── per-game timing ───────────────────────────────────────────────────────────

const tGames = nowMs();
const rows = [];
for (let i = 0; i < games; i += 1) {
  const t0 = nowMs();
  const packed = ex.run_game(i, args.goal);
  const ms = nowMs() - t0;
  rows.push({
    solver: `wasm-${variant}`,
    seed: i,
    goal: args.goal,
    success: Boolean(Number(packed & 1n)),
    evals: Number(packed >> 1n),
    ms,
  });
}
const perGameWallMs = nowMs() - tGames;

// ── batch path: one call for the whole set ────────────────────────────────────

const outPtr = ex.alloc(games * 2 * 4);
const batchTimes = [];
for (let rep = 0; rep < args.reps; rep += 1) {
  const t0 = nowMs();
  ex.solve_all(args.goal, outPtr, games);
  batchTimes.push(nowMs() - t0);
}
const batchOut = new Uint32Array(ex.memory.buffer, outPtr, games * 2);
let batchMismatch = 0;
for (let i = 0; i < games; i += 1) {
  const ok = Boolean(batchOut[i * 2]);
  const evals = batchOut[i * 2 + 1];
  if (ok !== rows[i].success || evals !== rows[i].evals) batchMismatch += 1;
}
batchTimes.sort((a, b) => a - b);

// ── cold-start cost of a fresh instance (what a terminated worker pays) ───────

const instantiations = [];
const warmStarts = [];
for (let i = 0; i < 5; i += 1) {
  const t0 = nowMs();
  const inst = await WebAssembly.instantiate(module);
  instantiations.push(nowMs() - t0);
  const instExp = inst.exports;
  // Emulate a worker booting: alloc + load games + first solve.
  const bytes = new TextEncoder().encode(gamesText);
  const ptr = instExp.alloc(bytes.length);
  new Uint8Array(instExp.memory.buffer).set(bytes, ptr);
  instExp.load_games(ptr, bytes.length);
  instExp.run_game(0, args.goal);
  warmStarts.push(nowMs() - t0);
}
instantiations.sort((a, b) => a - b);
warmStarts.sort((a, b) => a - b);

// ── optional cross-check against the Python run ──────────────────────────────

let check = null;
if (args.check) {
  const expected = new Map();
  for (const line of readFileSync(args.check, "utf8").split("\n")) {
    if (!line.trim()) continue;
    const seed = Number(/"seed":\s*(\d+)/.exec(line)[1]);
    const success = /"success":\s*(true|false)/.exec(line)[1] === "true";
    const evals = Number(/"evals":\s*(\d+)/.exec(line)[1]);
    expected.set(seed, { success, evals });
  }
  let successMismatch = 0;
  let evalMismatch = 0;
  for (const row of rows) {
    const e = expected.get(row.seed);
    if (!e) continue;
    if (e.success !== row.success) successMismatch += 1;
    if (e.evals !== row.evals) evalMismatch += 1;
  }
  check = { success_mismatches: successMismatch, eval_mismatches: evalMismatch };
}

// ── report ────────────────────────────────────────────────────────────────────

const evalsTotal = rows.reduce((a, r) => a + r.evals, 0);
const successes = rows.filter((r) => r.success).length;
const times = rows.map((r) => r.ms).sort((a, b) => a - b);
const summary = {
  solver: `wasm-${variant}`,
  wasm: args.wasm,
  wasm_bytes: wasmBytes.length,
  node: process.version,
  games,
  goal: args.goal,
  success_rate: successes / games,
  evals_total: evalsTotal,
  evals_per_s: (evalsTotal / perGameWallMs) * 1000,
  games_per_s: (games / perGameWallMs) * 1000,
  ms_median: quantile(times, 0.5),
  ms_mean: times.reduce((a, b) => a + b, 0) / times.length,
  ms_p90: quantile(times, 0.9),
  ms_total: perGameWallMs,
  batch_ms_median: batchTimes[Math.floor(batchTimes.length / 2)],
  batch_games_per_s: (games / batchTimes[Math.floor(batchTimes.length / 2)]) * 1000,
  batch_evals_per_s: (evalsTotal / batchTimes[Math.floor(batchTimes.length / 2)]) * 1000,
  batch_mismatch: batchMismatch,
  compile_ms: compileMs,
  first_instantiate_ms: firstInstantiateMs,
  instantiate_ms_median: instantiations[Math.floor(instantiations.length / 2)],
  warm_start_ms_median: warmStarts[Math.floor(warmStarts.length / 2)],
  simd_parity_boards: parityBoards,
  simd_parity_mismatches: parityBad,
  check,
};
console.log(JSON.stringify(summary, null, 2));

if (args.out) {
  mkdirSync(dirname(args.out), { recursive: true });
  writeFileSync(args.out, rows.map((r) => JSON.stringify(r)).join("\n") + "\n", "utf8");
  writeFileSync(args.out.replace(/\.jsonl$/, ".summary.json"), JSON.stringify(summary, null, 2), "utf8");
}
