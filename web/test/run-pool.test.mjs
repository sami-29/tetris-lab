// Seam 2: the Run's scheduler, with no DOM and no browser.
//
// `createRunPool` owns the pool, the chunk queue, the progress counters and the
// totals; it knows nothing about `Worker`, `document` or `hardwareConcurrency`, so
// this file drives it with a **fake worker that runs the real wasm in-process**
// (notes/15 §8.2). What that buys is the four properties the ticket names, each
// asserted here rather than by eye:
//
//   * every seed of the split is solved **exactly once** — the chunk intervals per
//     configuration are disjoint and contiguous, and their union is the split (P1);
//   * progress `{chunkDone, gamesDone, evals, ms}` is monotonic, so a slow machine
//     (a rising `ms` per game) is distinguishable from a stuck page (a frozen
//     counter and a rising clock) (P2);
//   * **stop** is `terminate()` on every worker, no chunk is applied afterwards,
//     and the pool respawns on the next Run (P3/P7);
//   * a **trap** costs one worker: it is terminated, logged, the chunk is retried
//     once on a fresh instance, and the rest of the pool keeps serving (P4).
//
// P5 is the one that makes the column trustworthy: a Run's totals over a reduced
// split equal a plain `gen_game` + `solve` loop over the same seeds, so the batch
// path is not a second implementation of the statistics.

import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { loadEngineFromBytes } from "../src/core/loader.ts";
import { solveChunk } from "../src/core/batch.ts";
import { createRunPool } from "../src/core/pool.ts";
import { percentile, statsOf } from "../src/core/run-stats.ts";
const ARTIFACT = fileURLToPath(new URL("../pkg/engine.wasm", import.meta.url));

const CONFIGS = [
  { id: 0, name: "goal8-p40-cap7", goal: 8, pieces: 40, cap: 7, beamWidth: 25, first: 0, last: 24 },
  { id: 1, name: "goal12-p100-cap7", goal: 12, pieces: 100, cap: 7, beamWidth: 25, first: 0, last: 24 },
];
const SPLIT = 25;

let compiled = null;
function artifact() {
  if (!existsSync(ARTIFACT)) {
    throw new Error(`missing ${ARTIFACT} — run \`node scripts/build-wasm.mjs\` first`);
  }
  compiled ??= WebAssembly.compile(new Uint8Array(readFileSync(ARTIFACT)));
  return compiled;
}

/**
 * A fake worker: it speaks the real message protocol and, in `real` mode, solves
 * with the real artifact through the real `solveChunk`. `script` can make one chunk
 * trap, and `perGameMs` swaps the engine for a synthetic summary so the chunk-size
 * arithmetic can be tested without solving a thousand games.
 */
function fakeSpawner({ mode = "real", perGameMs = null, trapFirst = null } = {}) {
  const spawned = [];
  const dispatched = [];
  const traps = new Map();

  const spawn = () => {
    const record = { terminated: 0, received: [] };
    const worker = {
      onmessage: null,
      postMessage(message) {
        record.received.push(message);
        if (message.type === "boot") {
          queueMicrotask(() => worker.onmessage?.({ data: { type: "ready", abiVersion: 2, scratchOffset: 0 } }));
          return;
        }
        if (message.type !== "run-chunk") return;
        dispatched.push({ configId: message.configId, firstSeed: message.firstSeed, count: message.count });
        const key = `${message.configId}:${message.firstSeed}`;
        const seen = (traps.get(key) ?? 0) + 1;
        traps.set(key, seen);
        if (trapFirst !== null && trapFirst(message, seen)) {
          queueMicrotask(() =>
            worker.onmessage?.({
              data: {
                type: "chunk-failed",
                runId: message.runId,
                chunkId: message.chunkId,
                configId: message.configId,
                firstSeed: message.firstSeed,
                count: message.count,
                code: "wasm-trap",
                message: "RuntimeError: unreachable",
              },
            }),
          );
          return;
        }
        void (async () => {
          if (mode === "synthetic") {
            const summaries = new Uint32Array(message.count * 4);
            const micros = new Uint32Array(message.count);
            for (let index = 0; index < message.count; index += 1) {
              summaries.set([1, 8, 10_000, 0], index * 4);
              micros[index] = Math.round(perGameMs * 1000);
            }
            worker.onmessage?.({
              data: {
                type: "chunk",
                runId: message.runId,
                chunkId: message.chunkId,
                configId: message.configId,
                firstSeed: message.firstSeed,
                count: message.count,
                summaries,
                micros,
                chunkMs: perGameMs * message.count,
              },
            });
            return;
          }
          const engine = await loadEngineFromBytes(await artifact());
          const result = solveChunk(engine, message, (games, evals, ms) => {
            worker.onmessage?.({
              data: {
                type: "run-progress",
                runId: message.runId,
                chunkId: message.chunkId,
                configId: message.configId,
                games,
                evals,
                ms,
              },
            });
          });
          if (!result.ok) {
            worker.onmessage?.({
              data: { type: "chunk-failed", ...message, code: result.code, message: result.message },
            });
            return;
          }
          worker.onmessage?.({
            data: {
              type: "chunk",
              runId: message.runId,
              chunkId: message.chunkId,
              configId: message.configId,
              firstSeed: message.firstSeed,
              count: message.count,
              summaries: result.summaries,
              micros: result.micros,
              chunkMs: result.chunkMs,
            },
          });
        })();
      },
      terminate() {
        record.terminated += 1;
      },
    };
    spawned.push({ worker, record });
    return worker;
  };

  return { spawn, spawned, dispatched };
}

/** Poll a predicate without a DOM: a handful of macrotasks. */
async function until(predicate, label, tries = 400) {
  for (let i = 0; i < tries; i += 1) {
    if (predicate()) return;
    await new Promise((resolve) => setTimeout(resolve, 1));
  }
  throw new Error(`timed out waiting for ${label}`);
}

function poolOf(fakes, options = {}) {
  const progress = [];
  const applied = [];
  const lines = [];
  const pool = createRunPool({
    spawn: fakes.spawn,
    workers: options.workers ?? 3,
    configs: options.configs ?? CONFIGS,
    onProgress: (value) => progress.push({ ...value }),
    onChunk: (message) => applied.push(message),
    onLog: (line) => lines.push(line),
    ...(options.extra ?? {}),
  });
  return { pool, progress, applied, lines };
}

test("P1: every seed is solved exactly once, and the union is the whole split", async () => {
  const fakes = fakeSpawner();
  const { pool, applied } = poolOf(fakes, { workers: 3 });
  const outcome = await pool.start();
  assert.equal(outcome.status, "complete");
  assert.equal(applied.length, outcome.chunks);

  for (const config of CONFIGS) {
    const intervals = applied
      .filter((message) => message.configId === config.id)
      .map((message) => [message.firstSeed, message.firstSeed + message.count - 1])
      .sort((a, b) => a[0] - b[0]);
    assert.ok(intervals.length > 0, `configuration ${config.id} was dispatched`);
    assert.equal(intervals[0][0], config.first, "starts at the first seed of the split");
    assert.equal(intervals.at(-1)[1], config.last, "ends at the last seed of the split");
    for (let index = 1; index < intervals.length; index += 1) {
      assert.equal(intervals[index][0], intervals[index - 1][1] + 1, "contiguous, so no seed is skipped");
    }
    const seeds = new Set();
    for (const [from, to] of intervals) for (let seed = from; seed <= to; seed += 1) seeds.add(seed);
    assert.equal(seeds.size, SPLIT, "every seed appears once — a set, not a count");
  }

  // No more workers than the cap, and every one of them was used.
  assert.ok(fakes.spawned.length <= 3, `${fakes.spawned.length} workers for a cap of 3`);
});

test("P2: progress is monotonic, and the last message is the whole split", async () => {
  const fakes = fakeSpawner();
  const { pool, progress, applied } = poolOf(fakes, { workers: 3 });
  const outcome = await pool.start();

  assert.ok(progress.length > 2, `progress is reported while running (${progress.length} updates)`);
  for (let index = 1; index < progress.length; index += 1) {
    const before = progress[index - 1];
    const now = progress[index];
    assert.ok(now.chunkDone >= before.chunkDone, `chunkDone never falls (${before.chunkDone} → ${now.chunkDone})`);
    assert.ok(now.gamesDone >= before.gamesDone, `gamesDone never falls (${before.gamesDone} → ${now.gamesDone})`);
    assert.ok(now.evals >= before.evals, `evals never fall (${before.evals} → ${now.evals})`);
    assert.ok(now.ms >= before.ms, `ms never falls (${before.ms} → ${now.ms})`);
  }
  const last = progress.at(-1);
  assert.equal(last.gamesDone, SPLIT * 2, "every game of both configurations");
  assert.equal(last.chunkDone, applied.length, "one chunk counted per applied chunk");
  assert.equal(outcome.progress.gamesDone, SPLIT * 2);
  assert.ok(last.evals > 0, "the progress object carries a throughput, not just a count");
  assert.ok(last.ms > 0, "and the work time");
});

test("P3: stop terminates every worker, applies nothing afterwards, and respawns lazily", async () => {
  const fakes = fakeSpawner();
  const { pool, applied } = poolOf(fakes, { workers: 2 });
  const running = pool.start();
  await until(() => applied.length >= 1, "the first chunk");
  pool.stop();
  const outcome = await running;

  assert.equal(outcome.status, "stopped", "a stopped Run is not a complete one");
  assert.equal(pool.size, 0, "every worker is gone");
  for (const { record } of fakes.spawned) {
    assert.equal(record.terminated, 1, "terminate() exactly once per worker");
  }
  const appliedAtStop = applied.length;
  const totalsAtStop = pool.totals().map((config) => config.n + config.missing);
  await new Promise((resolve) => setTimeout(resolve, 20));
  assert.equal(applied.length, appliedAtStop, "a chunk in flight when stop landed is not applied");
  assert.deepEqual(pool.totals().map((config) => config.n + config.missing), totalsAtStop);
  assert.ok(outcome.progress.gamesDone < SPLIT * 2, "and the result is honestly partial");

  // Lazy respawn: the next Run creates workers again and finishes.
  const second = await pool.start();
  assert.equal(second.status, "complete");
  assert.equal(second.progress.gamesDone, SPLIT * 2);
  assert.ok(fakes.spawned.length > 2, "the second Run spawned fresh workers");
});

test("P4: a trap costs one worker — terminated, logged, retried once, the pool keeps serving", async () => {
  const fakes = fakeSpawner({ trapFirst: (message, seen) => message.firstSeed === 8 && seen === 1 });
  const { pool, applied, lines } = poolOf(fakes, { workers: 3 });
  const outcome = await pool.start();

  assert.equal(outcome.status, "complete", "the Run finishes");
  assert.equal(outcome.traps, 1, "one trap");
  assert.equal(fakes.spawned.filter(({ record }) => record.terminated > 0).length, 1, "exactly one worker died");
  assert.equal(fakes.spawned.reduce((sum, { record }) => sum + record.terminated, 0), 1, "one terminate()");

  const retried = fakes.dispatched.filter((chunk) => chunk.firstSeed === 8);
  assert.equal(retried.length, 2, "the trapped chunk was dispatched twice — the retry");
  const at8 = applied.filter((message) => message.firstSeed === 8);
  assert.equal(at8.length, 2, "and applied once per configuration");
  assert.equal(
    applied.reduce((sum, message) => sum + message.count, 0),
    SPLIT * 2,
    "the retried games are counted once, so the split is still the split",
  );
  assert.ok(
    lines.some((line) => /wasm-trap/.test(line) && /respawn/.test(line)),
    `the trap is in the log: ${lines.join(" | ")}`,
  );
});

test("P5: a Run's totals equal a plain gen_game + solve loop over the same seeds", async () => {
  const fakes = fakeSpawner();
  const { pool } = poolOf(fakes, { workers: 2 });
  await pool.start();

  const engine = await loadEngineFromBytes(await artifact());
  for (const config of CONFIGS) {
    const totals = pool.totals().find((candidate) => candidate.shape.id === config.id);
    const stats = statsOf(totals);
    const evals = [];
    const failLines = [];
    let wins = 0;
    for (let seed = config.first; seed <= config.last; seed += 1) {
      assert.equal(engine.genGame(seed, config.pieces, config.cap), 0);
      const outcome = engine.solve(config.goal, config.beamWidth);
      assert.equal(outcome.status, 0);
      if (outcome.won) wins += 1;
      else failLines.push(outcome.lines);
      evals.push(outcome.evaluations);
    }
    evals.sort((a, b) => a - b);
    assert.equal(stats.n, evals.length, "n");
    assert.equal(stats.wins, wins, "wins");
    assert.equal(stats.nFail, failLines.length, "n_fail");
    assert.equal(stats.evalsP50, percentile(evals, 1, 2), "Evaluations p50");
    assert.equal(stats.evalsP90, percentile(evals, 9, 10), "Evaluations p90");
    const expectedMean = failLines.length
      ? failLines.reduce((sum, lines) => sum + lines, 0) / failLines.length
      : null;
    assert.equal(
      stats.failLinesMean === null ? null : stats.failLinesMean.toFixed(6),
      expectedMean === null ? null : expectedMean.toFixed(6),
      "mean Lines on failure",
    );
  }
});

test("P6: the chunk size comes from the measured per-game cost, not from a constant", async () => {
  const slow = fakeSpawner({ mode: "synthetic", perGameMs: 90 });
  const slowRun = poolOf(slow, { workers: 2 });
  await slowRun.pool.start();
  const slowSizes = slow.dispatched.map((chunk) => chunk.count);
  assert.equal(slowSizes[0], 4, "a four-game probe first");
  const slowProbes = slowSizes.filter((count) => count === 4).length;
  assert.ok(
    slowSizes.slice(slowProbes).every((count) => count === 1),
    `at 90 ms a game a chunk is one game, so an interactive solve waits ~90 ms: ${slowSizes.join(",")}`,
  );

  const fast = fakeSpawner({ mode: "synthetic", perGameMs: 2.8 });
  const fastRun = poolOf(fast, { workers: 2 });
  await fastRun.pool.start();
  const fastSizes = fast.dispatched.map((chunk) => chunk.count);
  assert.ok(Math.max(...fastSizes) >= 20, `at 2.8 ms a game chunks grow: ${fastSizes.join(",")}`);
  assert.ok(Math.max(...fastSizes) <= 200, "and are capped, so progress stays live");
});

test("P7: a chunk from a stopped Run's epoch changes nothing", async () => {
  const fakes = fakeSpawner();
  const { pool, applied } = poolOf(fakes, { workers: 2 });
  const running = pool.start();
  await until(() => applied.length >= 1, "the first chunk");
  pool.stop();
  await running;

  const before = pool.totals().map((config) => config.n + config.missing);
  const beforeProgress = { ...pool.progress };
  const late = applied[0];
  const zombie = fakes.spawned[0].worker;
  zombie.onmessage?.({
    data: {
      ...late,
      chunkId: late.chunkId + 999,
      summaries: new Uint32Array(late.count * 4).fill(1),
      micros: new Uint32Array(late.count).fill(2000),
    },
  });
  await new Promise((resolve) => setTimeout(resolve, 10));
  assert.deepEqual(pool.totals().map((config) => config.n + config.missing), before, "no game is added");
  assert.deepEqual(pool.progress, beforeProgress, "and progress does not move");
});
