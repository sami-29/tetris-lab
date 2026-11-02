// The solve worker: one wasm instance, one message loop.
//
// The page paints before this file has fetched anything, because the artifact is
// fetched *here* (§6.6 of the exploration notes: the wasm URL must be fetched in
// the worker, which is where the instance lives). Everything heavy — compiling,
// generating, solving — happens off the main thread, so the board's animation
// never stutters behind a solve.
//
// A trap is not a result. If a call throws, the instance is dropped, the failure is
// reported as `wasm-trap`, and the next request boots a fresh instance: §4.2's
// "respawn and log it", at the smallest possible scale.

import { AbiMismatchError, loadEngineFromUrl, statusName, type Engine } from "../core/loader.ts";
import type { FromWorker, ToWorker } from "../core/protocol.ts";

interface WorkerScope {
  postMessage(message: FromWorker): void;
  onmessage: ((event: MessageEvent<ToWorker>) => void) | null;
}

// SAFETY: a module worker's global is `DedicatedWorkerGlobalScope`, which the DOM
// lib types as `Window`; this is the two members of it this file uses, so the
// assertion states the invariant TypeScript cannot.
const scope: WorkerScope = self as unknown as WorkerScope;

let engineUrl: string | null = null;
let enginePromise: Promise<Engine> | null = null;

function post(message: FromWorker): void {
  scope.postMessage(message);
}

function failure(requestId: number, code: string, error: unknown): FromWorker {
  const message = error instanceof Error ? error.message : String(error);
  return { type: "error", requestId, code, message };
}

/** Boot (or re-boot, after a trap or a mismatch) the one instance. */
function engine(wasmUrl: string): Promise<Engine> {
  engineUrl = wasmUrl;
  if (enginePromise === null) {
    enginePromise = bootFresh(wasmUrl);
  }
  return enginePromise;
}

async function bootFresh(wasmUrl: string): Promise<Engine> {
  const loaded = await loadEngineFromUrl(wasmUrl);
  post({ type: "ready", abiVersion: loaded.abiVersion, scratchOffset: loaded.scratchOffset });
  return loaded;
}

/** A trap poisons the instance: forget it, so the next request gets a new one. */
function drop(): void {
  enginePromise = null;
}

scope.onmessage = async (event: MessageEvent<ToWorker>): Promise<void> => {
  const message = event.data;

  if (message.type === "boot") {
    try {
      await engine(message.wasmUrl);
    } catch (error) {
      drop();
      if (error instanceof AbiMismatchError) {
        // Fatal and not retried: a stale artifact is not a transient fault.
        post({ type: "error", requestId: 0, code: "abi-mismatch", message: error.message });
      } else {
        post(failure(0, "boot-failed", error));
      }
    }
    return;
  }

  const { requestId } = message;
  if (engineUrl === null) {
    post({ type: "error", requestId, code: "not-booted", message: "the worker got a request before boot" });
    return;
  }

  try {
    const loaded = await engine(engineUrl);

    const generated = loaded.genGame(message.seed, message.pieces, message.heightCap);
    if (generated !== 0) {
      post({
        type: "error",
        requestId,
        code: statusName(generated),
        message: `gen_game(${message.seed}, ${message.pieces}, ${message.heightCap}) returned ${generated} (${statusName(generated)})`,
      });
      return;
    }

    // Copy before solving: a view over wasm memory detaches on growth and cannot
    // be structured-cloned, so the message carries plain arrays.
    const board = Uint32Array.from(loaded.board());
    const sequence = loaded.sequence(message.pieces);

    const started = performance.now();
    // §2.3: the interactive solve is traced, the batch path is not. The band is
    // the request's, and the packed result is identical either way.
    const traced = typeof message.band === "number";
    const outcome = traced
      ? loaded.solveTraced(message.goal, message.beamWidth, message.band as number)
      : loaded.solve(message.goal, message.beamWidth);
    // Copy the trace out before the next call can overwrite the region.
    const trace = traced ? loaded.traceWords() : undefined;
    const elapsedMs = performance.now() - started;

    if (outcome.status !== 0) {
      post({
        type: "error",
        requestId,
        code: statusName(outcome.status),
        message: `solve(${message.goal}, ${message.beamWidth}) returned ${outcome.status} (${statusName(outcome.status)})`,
      });
      return;
    }

    const moves = Uint32Array.from(loaded.moves(outcome.moveCount));
    post({
      type: "game",
      requestId,
      input: {
        seed: message.seed,
        goal: message.goal,
        pieces: message.pieces,
        heightCap: message.heightCap,
        beamWidth: message.beamWidth,
      },
      board,
      sequence,
      outcome,
      moves,
      elapsedMs,
      trace,
      band: traced ? message.band : undefined,
    });
  } catch (error) {
    drop();
    post(failure(requestId, "wasm-trap", error));
  }
};
