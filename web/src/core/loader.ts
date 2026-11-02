// The one file in the app that mentions `WebAssembly`.
//
// Everything that can go wrong with wasm memory goes wrong in exactly one place,
// which is the point of the file existing at all (§2.2, ADR 0002):
//
//   * the `abi_version` handshake is checked once, here, before any buffer is
//     touched — a mismatch is a fatal typed error, not a silent misbehaviour;
//   * one `alloc` at boot reserves the variable-size region (§4's trace and batch
//     buffers are allocated once and never freed);
//   * **no view is cached across a call.** `memory.buffer` is replaced whenever the
//     engine grows memory, which detaches every existing `TypedArray`; so each
//     accessor below builds its view from the current `memory.buffer` and hands it
//     out. Callers must read what they need before the next call.
//
// The §4.1 buffers are fixed offsets into that memory, so the shapes here are
// constants from the design spec, not allocations:
//
//   0x00  board     10 × u32  column words, bit n of column c is row 19-n
//   0x40  sequence  200 bytes one ASCII letter per piece
//   0x140 moves     200 × u32 (piece_index << 16) | (rotation << 8) | column
//   0x460 end of the ABI region; nothing may map below this

import { DEFAULT_BEAM } from "../config/width.ts";

/** §4 row 1. Checked at boot; a mismatch is fatal. */
export const ABI_VERSION = 2;

export const BOARD_OFF = 0x00;
export const BOARD_WORDS = 10;
export const SEQ_OFF = 0x40;
export const SEQ_BYTES = 200;
export const MOVES_OFF = 0x140;
export const MOVES_MAX = 200;
export const ABI_END = 0x460;

/**
 * §2.3's traced band, and the only width this module needs: `DEFAULT_BEAM`, read
 * at build time from the committed width curve. The band never exceeds the beam
 * the page asks for, so a visitor who *widens* the beam still gets a legal trace —
 * the top `DEFAULT_BEAM` states of each Layer, with the parents it did not keep
 * named `-2` rather than left dangling.
 */
export const TRACE_BAND = DEFAULT_BEAM;

/**
 * §4.4's trace region: `band × pieces × 20` bytes, allocated **once at boot**
 * through `alloc` and never freed. `band` is `DEFAULT_BEAM` (§2.3: the band *is*
 * the beam) and `pieces` is the ABI's maximum (§5.8), so one region serves every
 * game the page can ask for. At the measured `DEFAULT_BEAM` of 25 that is 100,000
 * bytes, not the 12.8 MB the pre-measurement notes budgeted for.
 *
 * The region is also §4.4's *first* allocation, which is how `solve_traced` finds
 * it without a pointer argument (`engine-wasm/src/buffers.rs`), so nothing may be
 * allocated before it.
 */
export const TRACE_MAX_PIECES = 200;
export const TRACE_BYTES = TRACE_BAND * TRACE_MAX_PIECES * 20;

/** The artifact speaks a different ABI than this page: fatal, always. */
export class AbiMismatchError extends Error {
  readonly expected: number;
  readonly actual: number;

  constructor(expected: number, actual: number) {
    super(
      `abi_version mismatch: this page speaks ${expected}, the artifact reports ${actual}. ` +
        `The wasm is stale — rebuild it with \`npm run build:wasm\` and reload.`,
    );
    this.name = "AbiMismatchError";
    this.expected = expected;
    this.actual = actual;
  }
}

/** §4.2's packed `i64`, unpacked. `status !== 0` means the other fields are unspecified. */
export interface SolveOutcome {
  /** 0 ok, 1 out-of-domain, 2 bound exhausted, 3 illegal board, 4 illegal ABI state. */
  status: number;
  won: boolean;
  lines: number;
  evaluations: number;
  moveCount: number;
}

export interface Engine {
  readonly abiVersion: number;
  /** The boot `alloc` offset: §4.4's trace region. */
  readonly scratchOffset: number;
  /** The bytes the trace region can hold (`TRACE_BYTES`). */
  readonly traceBytes: number;
  /** §4 `gen_game`. 0 on success, otherwise a typed status code. */
  genGame(seed: number, pieces: number, heightCap: number): number;
  /** The Board at 0x00 as ten column words. A fresh view every call. */
  board(): Uint32Array;
  /** The Sequence at 0x40 as its letters. A fresh view every call. */
  sequence(count: number): string;
  /** §4 `solve`. Never throws for an expected failure; the status carries it. */
  solve(goal: number, beamWidth: number): SolveOutcome;
  /**
   * §4 `solve_traced`: `solve`, and §4.4's trace into the boot region. The packed
   * result is bit-identical to `solve`'s, asserted in `web/test/solve-traced.test.mjs`.
   */
  solveTraced(goal: number, beamWidth: number, band: number): SolveOutcome;
  /**
   * §4.4's trace words, copied out: `[n, ...5·n i32]`. A fresh copy every call,
   * because the region is overwritten by the next `solve_traced` and a view over
   * it may detach on a memory growth.
   */
  traceWords(): Int32Array;
  /** The Move words at 0x140, `count` of them. A fresh view every call. */
  moves(count: number): Uint32Array;
  /** §4 `alloc`: a byte offset, never freed. Exposed for the growth test. */
  allocBytes(len: number): number;
}

const STATUS_NAMES: Record<number, string> = {
  0: "ok",
  1: "out-of-domain",
  2: "bound-exhausted",
  3: "illegal-board",
  4: "illegal-abi-state",
};

/** A human sentence for a status code, for the page's error line. */
export function statusName(code: number): string {
  return STATUS_NAMES[code] ?? `unknown-status-${code}`;
}

interface Exports {
  memory: WebAssembly.Memory;
  abi_version(): number;
  alloc(len: number): number;
  gen_game(seed: number, pieces: number, heightCap: number): number;
  solve(goal: number, beamWidth: number): bigint;
  solve_traced(goal: number, beamWidth: number, band: number): bigint;
}

/**
 * §4.2's bit layout, one implementation. `BigInt.asUintN` because `solve` returns
 * an `i64` and JavaScript hands i64 back as a `BigInt`.
 */
export function unpackResult(raw: bigint | number): SolveOutcome {
  const packed = BigInt.asUintN(64, BigInt(raw));
  return {
    won: (packed & 1n) === 1n,
    evaluations: Number((packed >> 1n) & 0xffff_ffffn),
    status: Number((packed >> 33n) & 0xffn),
    lines: Number((packed >> 41n) & 0xffn),
    moveCount: Number((packed >> 49n) & 0x1ffn),
  };
}

const HANDSHAKE_EXPORTS = ["memory", "abi_version"] as const;
const ABI_EXPORTS = ["alloc", "gen_game", "solve", "solve_traced"] as const;

/**
 * Instantiate a module and hand back the loader's engine.
 *
 * The version check runs *before* the boot `alloc`, because a module that speaks a
 * different ABI may lay its buffers out differently — the only safe thing to do
 * with it is refuse it.
 */
function boot(instance: WebAssembly.Instance): Engine {
  // SAFETY: `instance.exports` is typed `Record<string, ExportValue>`, which no
  // TypeScript type can narrow to this module's exports. The invariant is checked
  // on the next line — every REQUIRED_EXPORTS name must be present — and the
  // `abi_version` handshake right after it proves the layouts match this file.
  const exports = instance.exports as unknown as Partial<Exports>;

  for (const name of HANDSHAKE_EXPORTS) {
    if (exports[name] === undefined) {
      throw new Error(`the artifact does not export \`${name}\` — is it engine-wasm.wasm?`);
    }
  }

  // The handshake comes first, and the rest of the export list only means
  // anything after it: a module that speaks another ABI may export other symbols,
  // and calling `alloc` on it would write to addresses this file only *thinks* it
  // knows.
  const abiVersion = exports.abi_version!();
  if (abiVersion !== ABI_VERSION) {
    throw new AbiMismatchError(ABI_VERSION, abiVersion);
  }

  for (const name of ABI_EXPORTS) {
    if (exports[name] === undefined) {
      throw new Error(`the artifact does not export \`${name}\` — is it engine-wasm.wasm?`);
    }
  }

  const allocBytes = (len: number): number => exports.alloc!(len);
  const scratchOffset = allocBytes(TRACE_BYTES);

  // Every accessor rebuilds its view: memory may have grown since the last call,
  // and a view taken across a growth has a byteLength of 0.
  const memory = (): WebAssembly.Memory => exports.memory!;
  const words = (offset: number, length: number): Uint32Array =>
    new Uint32Array(memory().buffer, offset, length);
  const bytes = (offset: number, length: number): Uint8Array =>
    new Uint8Array(memory().buffer, offset, length);

  return {
    abiVersion,
    scratchOffset,
    traceBytes: TRACE_BYTES,

    genGame(seed, pieces, heightCap) {
      return exports.gen_game!(seed, pieces, heightCap);
    },

    board() {
      return words(BOARD_OFF, BOARD_WORDS);
    },

    sequence(count) {
      const length = Math.max(0, Math.min(count, SEQ_BYTES));
      return String.fromCharCode(...bytes(SEQ_OFF, length));
    },

    solve(goal, beamWidth) {
      return unpackResult(exports.solve!(goal, beamWidth));
    },

    solveTraced(goal, beamWidth, band) {
      return unpackResult(exports.solve_traced!(goal, beamWidth, band));
    },

    traceWords() {
      // A fresh view, read after the call and copied out: `memory.buffer` is
      // replaced on growth, and the region is overwritten by the next traced
      // solve (§4.1's "never cache a TypedArray across a call").
      const base = scratchOffset >>> 2;
      const memoryWords = new Int32Array(memory().buffer);
      const count = memoryWords[base];
      if (count < 0 || 4 + 20 * count > TRACE_BYTES) {
        // A count the region cannot hold means the trace is not this instance's:
        // report an empty trace rather than read past the allocation.
        return new Int32Array(1);
      }
      return memoryWords.slice(base, base + 1 + 5 * count);
    },

    moves(count) {
      return words(MOVES_OFF, Math.max(0, Math.min(count, MOVES_MAX)));
    },

    allocBytes,
  };
}

/** Compile from bytes: the node test's path, and the streaming fallback's. */
export async function loadEngineFromBytes(
  source: Uint8Array | ArrayBuffer | WebAssembly.Module,
): Promise<Engine> {
  const module =
    source instanceof WebAssembly.Module
      ? source
      : await WebAssembly.compile(source as BufferSource);
  const instance = await WebAssembly.instantiate(module, {});
  return boot(instance);
}

/**
 * Compile from a URL: the browser's path. `compileStreaming` needs the response to
 * be served as `application/wasm`, which a dev server (or a wrong MIME from a
 * static host) may not do, so a rejection falls back to a buffer compile rather
 * than failing the page. A handshake failure is not a MIME problem and is never
 * retried.
 */
export async function loadEngineFromUrl(url: string | URL): Promise<Engine> {
  try {
    const module = await WebAssembly.compileStreaming(fetch(url));
    return boot(await WebAssembly.instantiate(module, {}));
  } catch (error) {
    if (error instanceof AbiMismatchError) throw error;
    const response = await fetch(url);
    if (!response.ok) {
      throw new Error(`could not fetch ${String(url)}: ${response.status} ${response.statusText}`);
    }
    return loadEngineFromBytes(new Uint8Array(await response.arrayBuffer()));
  }
}
