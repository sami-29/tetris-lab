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
}

export type ToWorker = BootRequest | SolveRequest;

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
}

/** A failure worth showing: a status code, a trap, or a mismatched ABI. */
export interface FailureMessage {
  type: "error";
  requestId: number;
  /** `abi-mismatch`, `wasm-trap`, or a §4.2 status name. */
  code: string;
  message: string;
}

export type FromWorker = ReadyMessage | GameMessage | FailureMessage;
