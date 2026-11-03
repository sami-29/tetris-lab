// One chunk of a Run, solved inside a worker.
//
// This is the only place that calls §4's `solve_batch`, and it calls it **once per
// game** — `count = 1` — for one measured reason: the engine cannot time itself
// (`std::time::Instant::now()` traps on `wasm32-unknown-unknown`), so the per-game ms
// the panel shows at median and p90 can only come from `performance.now()` around a
// single game's call. The export keeps its `count` argument for callers that do not
// need that row; a chunk here is a *JS-side* scheduling unit, not a wasm one, and the
// JS→wasm call costs ~33 ns against ~2.8 ms of work.
//
// Everything stays in typed arrays:
//
//   * `summaries` — §4.3's stride-4 words, one game after another, read through a
//     **fresh** view after every call (a solve can grow memory and detach the old
//     buffer);
//   * `micros` — one `u32` of wall clock per game, saturated at `u32::MAX` µs;
//   * both are transferred, never structured-cloned, and no per-game object is ever
//     built.
//
// It is DOM-free and worker-free so the scheduler's tests can run it in-process.

import type { Engine } from "./loader.ts";
import type { RunChunkRequest } from "./protocol.ts";

/** What a chunk produced, or why it produced nothing. */
export type ChunkResult =
  | { readonly ok: true; readonly summaries: Uint32Array; readonly micros: Uint32Array; readonly chunkMs: number }
  | { readonly ok: false; readonly code: "short-write"; readonly message: string };

/** Progress inside a chunk: games finished, their Evaluations, and the work time so far. */
export type ChunkProgress = (games: number, evaluations: number, workMs: number) => void;

const MICROS_MAX = 0xffff_ffff;

/**
 * Solve `request.count` games from `request.firstSeed`, measuring each one.
 *
 * A trap is **not** caught here: a wasm `RuntimeError` means the instance is dead and
 * the worker that owns it must respawn (§4.2), which is a decision for the layer that
 * owns the worker.
 */
export function solveChunk(
  engine: Engine,
  request: RunChunkRequest,
  onProgress?: ChunkProgress,
): ChunkResult {
  const { count, firstSeed, goal, pieces, heightCap, beamWidth } = request;
  const out = engine.allocBatch(count);
  const summaries = new Uint32Array(count * 4);
  const micros = new Uint32Array(count);
  // One progress message every fifth game and one at the end: a chunk of 28 games is
  // ~6 messages, so the rail stays alive without a per-game chat on the wire.
  const every = Math.max(1, Math.floor(count / 5));

  let evaluations = 0;
  const startedAt = performance.now();
  for (let index = 0; index < count; index += 1) {
    const seed = (firstSeed + index) >>> 0;
    const callStarted = performance.now();
    const written = engine.solveBatch(seed, 1, goal, pieces, heightCap, beamWidth, out);
    const elapsed = performance.now() - callStarted;
    if (written !== 1) {
      // The export writes nothing and returns 0 for an invalid request, so this is a
      // parameter problem and not a game the engine could not answer: the Run must
      // stop with a named reason instead of publishing a split full of `status: 1`.
      return {
        ok: false,
        code: "short-write",
        message:
          `solve_batch(${seed}, 1, goal ${goal}, ${pieces} pieces, cap ${heightCap}, beam ${beamWidth}) ` +
          `wrote ${written} summaries — the request is outside the ABI's domain`,
      };
    }
    // A fresh view *after* the call: memory may have grown, which detaches any view
    // taken before it, and the region is reused by the next game.
    const words = engine.summaries(out, 1);
    summaries[index * 4] = words[0];
    summaries[index * 4 + 1] = words[1];
    summaries[index * 4 + 2] = words[2];
    summaries[index * 4 + 3] = words[3];
    if (words[3] === 0) evaluations += words[2];
    micros[index] = Math.min(MICROS_MAX, Math.max(0, Math.round(elapsed * 1000)));

    const done = index + 1;
    if (onProgress && (done % every === 0 || done === count)) {
      onProgress(done, evaluations, performance.now() - startedAt);
    }
  }

  return { ok: true, summaries, micros, chunkMs: performance.now() - startedAt };
}
