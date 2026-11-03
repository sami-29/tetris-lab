// The typed message protocol between the page and the solve worker.
//
// Type-only: nothing here runs, and nothing here mentions `WebAssembly`. The
// board and the move list cross as plain `Uint32Array`s — copies, because a view
// over wasm memory cannot be transferred and would detach under the caller.

import type { SolveOutcome } from "./loader.ts";

/** Everything that identifies a game, in one place so a message can echo it. */
export interface GameInput {
  seed: number;
  goal: number;
  pieces: number;
  heightCap: number;
  beamWidth: number;
}

/** Boot the worker: fetch and compile the artifact there, not on the page. */
export interface BootRequest {
  type: "boot";
  /** The artifact's URL. The worker fetches it, so the page paints first. */
  wasmUrl: string;
}

/** Generate a game and solve it. One request, one answer. */
export interface SolveRequest extends GameInput {
  type: "solve";
  /** Echoed back so a stale answer can be dropped by the page. */
  requestId: number;
  /**
   * §2.3's band: ask for `solve_traced` at this many records per Layer. Absent
   * means a plain `solve` (the Run path needs no trace), which is why it is
   * optional rather than a second message type.
   */
  band?: number;
}

// ── the Run: one chunk of the split per message ──────────────────────────────
//
// §7's rule for this channel is "typed messages, explicit transfer lists, batch
// results as one `Uint32Array` of fixed-stride summaries". So a chunk request names
// an interval of Seeds and the behaviour to solve it at, and the answer is the raw
// §4.3 words plus one `u32` of measured time per game — no per-game objects, and no
// JSON, at any point.

/** One chunk: `count` consecutive Seeds from `firstSeed`, in one configuration. */
export interface RunChunkRequest {
  type: "run-chunk";
  /** Epoch: a stopped Run's late answers are dropped by comparing this. */
  runId: number;
  /** Unique per chunk, echoed back so an answer is matched to its work. */
  chunkId: number;
  /** 0 or 1: which committed configuration. */
  configId: 0 | 1;
  firstSeed: number;
  count: number;
  goal: number;
  pieces: number;
  heightCap: number;
  beamWidth: number;
}

/** A chunk finished: §4.3's summaries and this machine's per-game clock. */
export interface ChunkMessage {
  type: "chunk";
  runId: number;
  chunkId: number;
  configId: 0 | 1;
  firstSeed: number;
  count: number;
  /** `4 * count` words: won, lines, evaluations, status per game. Transferred. */
  summaries: Uint32Array;
  /** `count` microseconds of wall clock per game, measured here. Transferred. */
  micros: Uint32Array;
  /** The chunk's own work time in ms, which is what the progress object's `ms` sums. */
  chunkMs: number;
}

/** Liveness inside a chunk, so a slow machine is distinguishable from a stuck one. */
export interface RunProgressMessage {
  type: "run-progress";
  runId: number;
  chunkId: number;
  configId: 0 | 1;
  /** Games finished inside this chunk so far. */
  games: number;
  /** Σ `evaluations` of those games (`status` 0 only). */
  evals: number;
  /** This chunk's work time so far, in ms. */
  ms: number;
}

/** A chunk that produced no answer. `wasm-trap` is a dead instance, not a result. */
export interface ChunkFailedMessage {
  type: "chunk-failed";
  runId: number;
  chunkId: number;
  configId: 0 | 1;
  firstSeed: number;
  count: number;
  /** `wasm-trap` (respawn and retry once) or `short-write` (bad parameters: stop). */
  code: "wasm-trap" | "short-write" | "not-booted";
  message: string;
}

export type ToWorker = BootRequest | SolveRequest | RunChunkRequest;

/** The handshake passed and the worker is ready for requests. */
export interface ReadyMessage {
  type: "ready";
  abiVersion: number;
  /** The boot `alloc` offset, for the status line. */
  scratchOffset: number;
}

/** A game, its solve, and the pieces needed to play it back. */
export interface GameMessage {
  type: "game";
  requestId: number;
  input: GameInput;
  /** The Board at 0x00: ten column words, bit n of column c is row 19-n. */
  board: Uint32Array;
  /** The Sequence at 0x40. */
  sequence: string;
  outcome: SolveOutcome;
  /** §4.1.1 move words: (piece_index << 16) | (rotation << 8) | column. */
  moves: Uint32Array;
  /** The `solve` call's wall clock, for the status line. */
  elapsedMs: number;
  /**
   * §4.4's trace words, `[n, ...5·n i32]`, when the request asked for a band. A
   * copy, taken in the worker before anything else can overwrite the region.
   */
  trace?: Int32Array;
  /** The band the trace was recorded at, echoed for the labels. */
  band?: number;
}

/** A failure worth showing: a status code, a trap, or a mismatched ABI. */
export interface FailureMessage {
  type: "error";
  requestId: number;
  /** `abi-mismatch`, `wasm-trap`, or a §4.2 status name. */
  code: string;
  message: string;
}

export type FromWorker =
  | ReadyMessage
  | GameMessage
  | FailureMessage
  | ChunkMessage
  | RunProgressMessage
  | ChunkFailedMessage;
