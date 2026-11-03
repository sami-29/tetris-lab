// The Run's scheduler: a pool of workers, a chunk queue, and the totals.
//
// §7's shape, and nothing more than it:
//
//   * `N = clamp(hardwareConcurrency - 1, 1, 8)` module workers, one wasm instance
//     each, **no `SharedArrayBuffer` and no COOP/COEP**. The page is not
//     cross-origin isolated, which is also why every ms figure is quantised by the
//     browser's clock;
//   * chunks are dispatched over the split with one cursor per configuration, so the
//     union of applied chunks is the whole split and no Seed is solved twice;
//   * progress is `{chunkDone, gamesDone, evals, ms}` aggregated **on this thread**
//     from messages, because nothing can be polled out of a worker without a shared
//     buffer;
//   * **stop is `terminate()`** on every worker, idle ones included: the worker runs
//     a synchronous wasm call inside one message handler, so no message can
//     interrupt it. Workers respawn lazily, on the next Run;
//   * a trap costs **one** worker: it is terminated, logged, and its chunk is retried
//     once on a fresh instance. A second trap marks those games missing (never
//     losses) and the Run carries on; more than three traps abort it, because a
//     systematically trapping artifact cannot produce a truthful table.
//
// DOM-free and `Worker`-free on purpose: `spawn` is injected, so
// `web/test/run-pool.test.mjs` drives this with a fake worker that runs the real
// artifact in-process, and every property above is asserted there rather than by eye.

import { solveChunk } from "./batch.ts";
import type { Engine } from "./loader.ts";
import type {
  ChunkFailedMessage,
  ChunkMessage,
  FromWorker,
  RunChunkRequest,
  RunProgressMessage,
} from "./protocol.ts";
import {
  addMissing,
  addSummaries,
  emptyTotals,
  statsOf,
  type ConfigShape,
  type ConfigTotals,
  type ConfigStats,
} from "./run-stats.ts";

/** The minimum a worker has to be, so the scheduler never needs a DOM. */
export interface WorkerLike {
  postMessage(message: unknown): void;
  terminate(): void;
  onmessage: ((event: { data: FromWorker }) => void) | null;
}

/** The four counters §2.4 names, aggregated on this thread. */
export interface RunProgress {
  /** Chunks applied, both configurations. */
  chunkDone: number;
  /** Games measured or status-coded, both configurations. */
  gamesDone: number;
  /** Σ `evaluations` of the measured games. */
  evals: number;
  /** Σ the chunks' own work time, in ms — the same quantity a Reference row's `batch_ms` is. */
  ms: number;
}

export type RunStatus = "idle" | "running" | "complete" | "stopped" | "failed";

export interface RunOutcome {
  readonly status: Exclude<RunStatus, "idle" | "running">;
  readonly progress: RunProgress;
  readonly totals: readonly ConfigTotals[];
  readonly stats: readonly ConfigStats[];
  /** Traps that cost a worker, whether or not the retry answered. */
  readonly traps: number;
  /** Chunks applied. */
  readonly chunks: number;
  /** This thread's wall clock, which includes the message hops. */
  readonly elapsedMs: number;
  /** Why a failed Run failed. */
  readonly reason: string;
}

export interface RunPoolOptions {
  /**
   * Create one worker and start it booting. The pool never constructs a `Worker`
   * itself, so it can be tested without a browser.
   */
  readonly spawn: () => WorkerLike;
  /** `clamp(hardwareConcurrency - 1, 1, 8)`, decided by the caller. */
  readonly workers: number;
  /** The configurations to run, in the panel's order. Their `first`/`last` are the split. */
  readonly configs: readonly ConfigShape[];
  /** Target chunk length in ms: long enough to amortise messages, short enough that a
   * queued interactive solve waits ~one chunk. */
  readonly chunkTargetMs?: number;
  /** The largest chunk, so progress stays live on a fast machine. */
  readonly maxChunkGames?: number;
  /** The first chunk, before any per-game cost has been measured. */
  readonly probeGames?: number;
  /** `performance.now`, injected so a test can be deterministic. */
  readonly now?: () => number;
  readonly onProgress?: (progress: RunProgress) => void;
  readonly onChunk?: (message: ChunkMessage) => void;
  readonly onLog?: (line: string, tone?: "plain" | "error") => void;
}

export interface RunPool {
  /** The cap the pool was built with. */
  readonly workers: number;
  /** Live workers right now (0 after a stop, until the next Run). */
  readonly size: number;
  readonly progress: RunProgress;
  readonly status: RunStatus;
  /** The epoch; a message carrying another one is from a Run that no longer exists. */
  runId(): number;
  totals(): readonly ConfigTotals[];
  stats(): readonly ConfigStats[];
  start(): Promise<RunOutcome>;
  stop(): void;
}

interface Dispatched {
  readonly chunkId: number;
  readonly configId: 0 | 1;
  readonly firstSeed: number;
  readonly count: number;
  readonly games: number;
  readonly goal: number;
  readonly pieces: number;
  readonly heightCap: number;
  readonly beamWidth: number;
  attempts: number;
  /** Games and work time reported from inside the chunk, for a live progress object. */
  gamesIn: number;
  evalsIn: number;
  msIn: number;
}

interface Slot {
  readonly worker: WorkerLike;
  chunk: Dispatched | null;
  ready: boolean;
  /** Watchdog handle; `null` when the slot is idle. */
  timer: ReturnType<typeof setTimeout> | null;
}

const TRAP_ABORT = 3;

export function createRunPool(options: RunPoolOptions): RunPool {
  const now = options.now ?? (() => performance.now());
  const chunkTargetMs = options.chunkTargetMs ?? 80;
  const maxChunkGames = options.maxChunkGames ?? 200;
  const probeGames = Math.max(1, options.probeGames ?? 4);
  const workers = Math.max(1, Math.min(8, Math.floor(options.workers)));

  const log = (line: string, tone: "plain" | "error" = "plain"): void => options.onLog?.(line, tone);

  let epoch = 0;
  let status: RunStatus = "idle";
  let totals: ConfigTotals[] = options.configs.map((shape) => emptyTotals(shape));
  let progress: RunProgress = { chunkDone: 0, gamesDone: 0, evals: 0, ms: 0 };
  let slots: Slot[] = [];
  let cursors: number[] = options.configs.map((shape) => shape.first);
  let turn = 0;
  let nextChunkId = 0;
  let chunkGames = probeGames;
  let perGameMs: number | null = null;
  let slowestChunkMs = 0;
  let traps = 0;
  let chunksApplied = 0;
  let startedAt = 0;
  let finish: ((outcome: RunOutcome) => void) | null = null;
  let reason = "";

  /** Recompute the progress object, keeping every field monotone by construction. */
  const publish = (): void => {
    const appliedGames = totals.reduce((sum, config) => sum + config.n + config.missing, 0);
    const appliedEvals = totals.reduce((sum, config) => sum + config.evals.reduce((s, v) => s + v, 0), 0);
    const appliedMs = totals.reduce((sum, config) => sum + config.workMs, 0);
    const inGames = slots.reduce((sum, slot) => sum + (slot.chunk ? slot.chunk.gamesIn : 0), 0);
    const inEvals = slots.reduce((sum, slot) => sum + (slot.chunk ? slot.chunk.evalsIn : 0), 0);
    const inMs = slots.reduce((sum, slot) => sum + (slot.chunk ? slot.chunk.msIn : 0), 0);
    // `max` rather than assignment: a trapped chunk's partial progress disappears when
    // it is retried, and a counter that went backwards would read as a stuck page.
    progress = {
      chunkDone: Math.max(progress.chunkDone, chunksApplied),
      gamesDone: Math.max(progress.gamesDone, appliedGames + inGames),
      evals: Math.max(progress.evals, appliedEvals + inEvals),
      ms: Math.max(progress.ms, appliedMs + inMs),
    };
    options.onProgress?.({ ...progress });
  };

  const outcome = (final: Exclude<RunStatus, "idle" | "running">): RunOutcome => ({
    status: final,
    progress: { ...progress },
    totals: totals.map((config) => config),
    stats: totals.map((config) => statsOf(config)),
    traps,
    chunks: chunksApplied,
    elapsedMs: now() - startedAt,
    reason,
  });

  const finishWith = (final: Exclude<RunStatus, "idle" | "running">): void => {
    if (status !== "running") return;
    status = final;
    // The wall clock is only meaningful for a Run that ended on its own or was
    // stopped; a failed Run still reports what it spent.
    const done = finish;
    finish = null;
    for (const slot of slots) clearWatchdog(slot);
    done?.(outcome(final));
  };

  const clearWatchdog = (slot: Slot): void => {
    if (slot.timer !== null) {
      clearTimeout(slot.timer);
      slot.timer = null;
    }
  };

  const exhausted = (index: number): boolean =>
    cursors[index] > options.configs[index].last;

  const complete = (): boolean =>
    options.configs.every((_, index) => exhausted(index)) && slots.every((slot) => slot.chunk === null);

  /** The next chunk: round-robin over the configurations, so both columns fill together. */
  const nextChunk = (): Dispatched | null => {
    for (let attempt = 0; attempt < options.configs.length; attempt += 1) {
      const index = (turn + attempt) % options.configs.length;
      if (exhausted(index)) continue;
      const shape = options.configs[index];
      const count = Math.min(chunkGames, shape.last - cursors[index] + 1);
      const chunk: Dispatched = {
        chunkId: nextChunkId,
        configId: shape.id,
        firstSeed: cursors[index],
        count,
        games: count,
        goal: shape.goal,
        pieces: shape.pieces,
        heightCap: shape.cap,
        beamWidth: shape.beamWidth,
        attempts: 1,
        gamesIn: 0,
        evalsIn: 0,
        msIn: 0,
      };
      nextChunkId += 1;
      cursors[index] += count;
      turn = (index + 1) % options.configs.length;
      return chunk;
    }
    return null;
  };

  const post = (slot: Slot, chunk: Dispatched): void => {
    const request: RunChunkRequest = {
      type: "run-chunk",
      runId: epoch,
      chunkId: chunk.chunkId,
      configId: chunk.configId,
      firstSeed: chunk.firstSeed,
      count: chunk.count,
      goal: chunk.goal,
      pieces: chunk.pieces,
      heightCap: chunk.heightCap,
      beamWidth: chunk.beamWidth,
    };
    slot.chunk = chunk;
    chunk.gamesIn = 0;
    chunk.evalsIn = 0;
    chunk.msIn = 0;
    slot.worker.postMessage(request);
    // A silent worker is the one case a trap message cannot cover: a deadline turns
    // it into the same respawn-and-retry as a trap, and says so in the log.
    const deadline = Math.max(3000, 10 * slowestChunkMs);
    slot.timer = setTimeout(() => {
      const lost = slot.chunk;
      if (status !== "running" || lost === null) return;
      log(
        `chunk ${lost.chunkId} · seed ${lost.firstSeed} · no answer in ${Math.round(deadline)} ms ` +
          `(slowest chunk so far ${Math.round(slowestChunkMs)} ms) — respawned a worker`,
        "error",
      );
      retire(slot, lost);
    }, deadline);
    // SAFETY: `setTimeout` returns a `Timeout` under Node and a number in a browser.
    // Node's has `unref`, which a browser's number does not; asking an optional member
    // on the number is `undefined` and skipped. Without it a test that leaves a
    // watchdog armed would hold the process open.
    const handle = slot.timer as unknown as { unref?: () => void };
    handle?.unref?.();
  };

  const dispatch = (): void => {
    if (status !== "running") return;
    // Grow the pool only when there is work for a new worker, and never past the cap.
    for (;;) {
      const idle = slots.find((slot) => slot.ready && slot.chunk === null);
      if (idle) {
        const chunk = nextChunk();
        if (chunk === null) break;
        post(idle, chunk);
        continue;
      }
      // A busy worker is still a worker: the cap is on live slots, not on idle ones.
      // (Counting only idle slots spawned a new worker for every busy chunk.)
      const work = !options.configs.every((_, index) => exhausted(index));
      if (work && slots.length < workers) {
        slots.push(bootWorker());
        continue;
      }
      break;
    }
    if (complete()) finishWith("complete");
  };

  /** Kill one worker and forget it: `terminate()` is the only way to stop a chunk. */
  const retire = (slot: Slot, lost: Dispatched | null): void => {
    clearWatchdog(slot);
    try {
      slot.worker.terminate();
    } catch {
      // A worker that is already gone is exactly the state we wanted.
    }
    slots = slots.filter((candidate) => candidate !== slot);
    if (lost !== null) {
      // The chunk's games are dispatched again on a fresh instance — retried once,
      // then marked missing rather than guessed.
      slot.chunk = null;
      if (lost.attempts >= 2) {
        const index = options.configs.findIndex((shape) => shape.id === lost.configId);
        if (index !== -1) {
          addMissing(totals[index], lost.count);
          log(
            `chunk ${lost.chunkId} · seed ${lost.firstSeed} · ${lost.count} games produced no measurement after two attempts`,
            "error",
          );
        }
      } else {
        lost.attempts += 1;
        // Re-dispatch exactly this interval: put the cursor back only if nothing
        // else has taken these seeds (the cursor already moved past them).
        const index = options.configs.findIndex((shape) => shape.id === lost.configId);
        if (index !== -1 && lost.firstSeed === cursors[index] - lost.count) {
          cursors[index] = lost.firstSeed;
        } else {
          requeue.push(lost);
        }
      }
    }
    publish();
    dispatch();
  };

  /** A chunk that has to be re-posted even though the cursors have moved on. */
  const requeue: Dispatched[] = [];

  const bootWorker = (): Slot => {
    const slot: Slot = { worker: options.spawn(), chunk: null, ready: false, timer: null };
    slot.worker.onmessage = (event) => receive(slot, event.data);
    return slot;
  };

  const receive = (slot: Slot, message: FromWorker): void => {
    if (message.type === "ready") {
      slot.ready = true;
      dispatch();
      return;
    }
    if (message.type === "run-progress") {
      onProgressMessage(slot, message);
      return;
    }
    if (message.type === "chunk") {
      onChunkMessage(slot, message);
      return;
    }
    if (message.type === "chunk-failed") {
      onChunkFailed(slot, message);
      return;
    }
    if (message.type === "error" && message.requestId === 0) {
      log(`worker: ${message.code}: ${message.message}`, "error");
      const lost = slot.chunk;
      slot.chunk = null;
      retire(slot, lost);
    }
  };

  const onProgressMessage = (slot: Slot, message: RunProgressMessage): void => {
    if (status !== "running" || message.runId !== epoch) return;
    const chunk = slot.chunk;
    if (chunk === null || chunk.chunkId !== message.chunkId) return;
    chunk.gamesIn = Math.max(chunk.gamesIn, message.games);
    chunk.evalsIn = Math.max(chunk.evalsIn, message.evals);
    chunk.msIn = Math.max(chunk.msIn, message.ms);
    publish();
  };

  const onChunkMessage = (slot: Slot, message: ChunkMessage): void => {
    // Epoch first: a stopped Run's late answer must change nothing at all.
    if (status !== "running" || message.runId !== epoch) return;
    const chunk = slot.chunk;
    if (chunk === null || chunk.chunkId !== message.chunkId) return;
    clearWatchdog(slot);
    slot.chunk = null;

    const index = options.configs.findIndex((shape) => shape.id === message.configId);
    if (index !== -1) {
      addSummaries(totals[index], message.summaries, message.micros, message.chunkMs);
      chunksApplied += 1;
      slowestChunkMs = Math.max(slowestChunkMs, message.chunkMs);
      const micros = message.micros;
      if (micros.length > 0) {
        const mean = micros.reduce((sum, value) => sum + value, 0) / micros.length / 1000;
        perGameMs = perGameMs === null ? mean : perGameMs * 0.5 + mean * 0.5;
        // The chunk length follows the measurement, never a constant: at 2.8 ms a game
        // a chunk is ~28 games (an interactive solve waits one chunk at most), and at
        // 90 ms a game it collapses to one.
        chunkGames = Math.max(1, Math.min(maxChunkGames, Math.round(chunkTargetMs / Math.max(0.001, perGameMs))));
      }
    }
    options.onChunk?.(message);
    publish();
    // A requeued retry goes first: it is the chunk that already cost a worker.
    const retry = requeue.shift();
    if (retry) {
      post(slot, retry);
      return;
    }
    dispatch();
  };

  const onChunkFailed = (slot: Slot, message: ChunkFailedMessage): void => {
    if (status !== "running" || message.runId !== epoch) return;
    const chunk = slot.chunk;
    if (chunk === null || chunk.chunkId !== message.chunkId) return;
    if (message.code === "short-write" || message.code === "not-booted") {
      // A parameter problem is not a game the engine could not answer: publishing a
      // split of missing games would be a false sentence about the games.
      reason = message.message;
      log(`chunk ${message.chunkId}: ${message.message}`, "error");
      slot.chunk = null;
      retire(slot, null);
      status = "running";
      finishWith("failed");
      return;
    }
    traps += 1;
    log(
      `chunk ${message.chunkId} · seed ${message.firstSeed} · wasm-trap · respawned a worker ` +
        `(${message.message})`,
      "error",
    );
    if (traps > TRAP_ABORT) {
      reason = `the artifact trapped on ${traps} chunks — this build cannot answer on this machine`;
      slot.chunk = null;
      retire(slot, null);
      finishWith("failed");
      return;
    }
    retire(slot, chunk);
  };

  return {
    workers,
    get size() {
      return slots.length;
    },
    get progress() {
      return { ...progress };
    },
    get status() {
      return status;
    },
    runId: () => epoch,
    totals: () => totals.map((config) => config),
    stats: () => totals.map((config) => statsOf(config)),
    start() {
      if (status === "running") return Promise.reject(new Error("a Run is already in flight"));
      epoch += 1;
      status = "running";
      reason = "";
      traps = 0;
      chunksApplied = 0;
      nextChunkId = 0;
      slowestChunkMs = 0;
      chunkGames = probeGames;
      perGameMs = null;
      turn = 0;
      cursors = options.configs.map((shape) => shape.first);
      totals = options.configs.map((shape) => emptyTotals(shape));
      progress = { chunkDone: 0, gamesDone: 0, evals: 0, ms: 0 };
      requeue.length = 0;
      slots = [];
      startedAt = now();
      const done = new Promise<RunOutcome>((resolve) => {
        finish = resolve;
      });
      // One worker now; `dispatch` grows the pool as chunks become available, so a
      // one-chunk Run does not pay for eight boots.
      slots.push(bootWorker());
      publish();
      dispatch();
      return done;
    },
    stop() {
      if (status !== "running") return;
      // The epoch first: every answer still on the wire is now stale, and `receive`
      // drops it before it can add a game to a Run that no longer exists.
      epoch += 1;
      for (const slot of slots) {
        clearWatchdog(slot);
        try {
          slot.worker.terminate();
        } catch {
          // already gone
        }
        slot.chunk = null;
      }
      slots = [];
      requeue.length = 0;
      status = "stopped";
      const done = finish;
      finish = null;
      log(`Run stopped at ${progress.gamesDone} of ${options.configs.reduce((sum, shape) => sum + (shape.last - shape.first + 1), 0)} games`);
      done?.(outcome("stopped"));
    },
  };
}

/**
 * The pool size §7 specifies: one worker fewer than the machine's logical cores, so
 * the page keeps a core, clamped to 1…8 so a 32-thread box does not spawn 31.
 */
export function poolSize(cores: number | undefined): number {
  const reported = typeof cores === "number" && Number.isFinite(cores) ? cores : 1;
  return Math.max(1, Math.min(8, Math.floor(reported) - 1));
}

/** The worker's side of the chunk protocol, shared by the worker file and the tests. */
export function runChunkWith(
  engine: Engine,
  request: RunChunkRequest,
  onProgress: (games: number, evals: number, ms: number) => void,
): ChunkMessage | ChunkFailedMessage {
  const result = solveChunk(engine, request, onProgress);
  if (!result.ok) {
    return {
      type: "chunk-failed",
      runId: request.runId,
      chunkId: request.chunkId,
      configId: request.configId,
      firstSeed: request.firstSeed,
      count: request.count,
      code: result.code,
      message: result.message,
    };
  }
  return {
    type: "chunk",
    runId: request.runId,
    chunkId: request.chunkId,
    configId: request.configId,
    firstSeed: request.firstSeed,
    count: request.count,
    summaries: result.summaries,
    micros: result.micros,
    chunkMs: result.chunkMs,
  };
}
