// Seam 2: the real wasm artifact through the real loader.
//
//   node scripts/build-wasm.mjs && node --test test/
//
// Nothing here is simulated: the bytes come out of `cargo build --release
// --target wasm32-unknown-unknown`, go through `web/src/core/loader.ts` — the
// only file in the app that mentions `WebAssembly` — and come back as a Board,
// a Sequence and a Move list. Two of these tests are guards against faults that
// are invisible in a native test:
//
//   * the §4 buffers live at 0x00/0x40/0x140, but this toolchain puts the data
//     segment and the shadow stack in the same address range, so a build flag or
//     a stray static can silently overlap them (assert: no active data segment
//     reaches below 0x460, and the buffers are zero at boot);
//   * every fixed-offset access must be volatile, because a plain store to
//     offset 0 is UB (a plain store was observed writing nothing at all), so a
//     pass here proves the store actually landed in memory the loader can see.

import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { ABI_VERSION, AbiMismatchError, loadEngineFromBytes } from "../src/core/loader.ts";

const ARTIFACT = fileURLToPath(new URL("../pkg/engine.wasm", import.meta.url));

/** §4.1: the three static buffers, and the first byte no §4 buffer may touch. */
const BOARD_OFF = 0x00;
const SEQ_OFF = 0x40;
const MOVES_OFF = 0x140;
const ABI_END = 0x460;

/** The golden games of `engine/tests/fixtures/games.txt`, seed 42. */
const SEED_42_BOARD = [0x00, 0x1f, 0x1f, 0x04, 0x64, 0x50, 0x7e, 0x03, 0x00, 0x00];
const SEED_42_SEQUENCE = "OJTISZLSTIZOLJSIZTJLOSTJILZOTZSLIOJISZLO";

function wasmBytes() {
  if (!existsSync(ARTIFACT)) {
    throw new Error(`missing ${ARTIFACT} — run \`node scripts/build-wasm.mjs\` first`);
  }
  return new Uint8Array(readFileSync(ARTIFACT));
}

// ── a hand-built module with the wrong abi_version ──────────────────────────
//
// The handshake is only worth having if a mismatched module is refused, so the
// mismatch is tested with a real wasm module that really exports a different
// number — not with a stub object that the loader might be treating specially.

function uleb(value) {
  const out = [];
  let v = value;
  do {
    let byte = v & 0x7f;
    v >>>= 7;
    if (v !== 0) byte |= 0x80;
    out.push(byte);
  } while (v !== 0);
  return out;
}

function section(id, payload) {
  return [id, ...uleb(payload.length), ...payload];
}

function nameBytes(text) {
  const bytes = [...Buffer.from(text, "utf8")];
  return [...uleb(bytes.length), ...bytes];
}

/** A module exporting `memory` (1 page) and `abi_version() -> version`. */
function moduleWithAbiVersion(version) {
  const typeSection = section(1, [...uleb(1), 0x60, ...uleb(0), ...uleb(1), 0x7f]);
  const funcSection = section(3, [...uleb(1), ...uleb(0)]);
  const memSection = section(5, [...uleb(1), 0x00, ...uleb(1)]);
  const exports = [
    [...uleb(2)],
    [...nameBytes("memory"), 0x02, ...uleb(0)],
    [...nameBytes("abi_version"), 0x00, ...uleb(0)],
  ].flat();
  const exportSection = section(7, exports);
  const body = [0x00, 0x41, ...uleb(version), 0x0b]; // no locals; i32.const; end
  const codeSection = section(10, [...uleb(1), ...uleb(body.length), ...body]);
  return new Uint8Array([
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00,
    ...typeSection,
    ...funcSection,
    ...memSection,
    ...exportSection,
    ...codeSection,
  ]);
}

// ── the wasm container, read the way a gate would read it ───────────────────

function dataSegments(bytes) {
  const readU = (p) => {
    let result = 0;
    let shift = 0;
    let byte;
    do {
      byte = bytes[p.i++];
      result |= (byte & 0x7f) << shift;
      shift += 7;
    } while (byte & 0x80);
    return result >>> 0;
  };
  const readS = (p) => {
    let result = 0;
    let shift = 0;
    let byte;
    do {
      byte = bytes[p.i++];
      result |= (byte & 0x7f) << shift;
      shift += 7;
    } while (byte & 0x80);
    if (byte & 0x40) result |= -1 << shift;
    return result;
  };
  // An active segment's offset is an i32.const expression (opcode 0x41) or a
  // global.get (0x23); everything else in a data section is content.
  const offsetExpr = (p) => {
    const op = bytes[p.i++];
    if (op === 0x41) {
      const value = readS(p);
      assert.equal(bytes[p.i++], 0x0b, "offset expression must end");
      return { kind: "active", offset: value };
    }
    if (op === 0x23) {
      readU(p);
      assert.equal(bytes[p.i++], 0x0b, "offset expression must end");
      return { kind: "global", offset: null };
    }
    throw new Error(`unexpected offset expression opcode 0x${op.toString(16)}`);
  };

  const cursor = { i: 8 };
  const segments = [];
  while (cursor.i < bytes.length) {
    const id = bytes[cursor.i++];
    const size = readU(cursor);
    const end = cursor.i + size;
    if (id === 11) {
      const count = readU(cursor);
      for (let n = 0; n < count; n += 1) {
        const flags = readU(cursor);
        let where;
        if (flags === 0) {
          where = offsetExpr(cursor);
        } else if (flags === 2) {
          readU(cursor); // memory index
          where = offsetExpr(cursor);
        } else {
          where = { kind: "passive", offset: null };
        }
        const length = readU(cursor);
        cursor.i += length;
        segments.push({ flags, offset: where.offset, length });
      }
    }
    cursor.i = end;
  }
  return segments;
}

// ── the tests ──────────────────────────────────────────────────────────────

test("the loader's handshake reports abi_version 2", async () => {
  const engine = await loadEngineFromBytes(wasmBytes());
  assert.equal(ABI_VERSION, 2);
  assert.equal(engine.abiVersion, 2);
});

test("a mismatched abi_version is fatal, not silent", async () => {
  await assert.rejects(
    () => loadEngineFromBytes(moduleWithAbiVersion(3)),
    (err) => {
      assert.ok(err instanceof AbiMismatchError, `expected AbiMismatchError, got ${err}`);
      assert.equal(err.expected, 2);
      assert.equal(err.actual, 3);
      return true;
    },
  );
  // An older version is equally fatal: the buffer layout changed with it.
  await assert.rejects(() => loadEngineFromBytes(moduleWithAbiVersion(1)), AbiMismatchError);
});

test("a known Seed generates the committed game and solves it", async () => {
  const engine = await loadEngineFromBytes(wasmBytes());

  assert.equal(engine.genGame(42, 40, 7), 0, "gen_game returns 0 on success");
  assert.deepEqual([...engine.board()], SEED_42_BOARD, "the Board crossed at 0x00");
  assert.equal(engine.sequence(40), SEED_42_SEQUENCE, "the Sequence crossed at 0x40");

  const outcome = engine.solve(8, 25);
  assert.equal(outcome.status, 0, "status 0");
  assert.equal(outcome.won, true, "seed 42 clears 8 lines at width 25");
  assert.ok(outcome.lines >= 8, `lines ${outcome.lines} >= goal 8`);
  assert.ok(outcome.evaluations > 0, "a solve counts evaluations");
  assert.ok(outcome.moveCount > 0 && outcome.moveCount <= 40, `moveCount ${outcome.moveCount}`);

  const moves = engine.moves(outcome.moveCount);
  assert.equal(moves.length, outcome.moveCount);
  for (const word of moves) {
    const rotation = (word >>> 8) & 0xff;
    const column = word & 0xff;
    assert.ok(rotation <= 3, `rotation ${rotation} fits a byte`);
    assert.ok(column <= 9, `column ${column} is on the field`);
  }

  // A solve must not disturb the game it was given: the loader reads the same
  // board back out of the same buffer.
  assert.deepEqual([...engine.board()], SEED_42_BOARD);
});

test("the outcome is not recomputed in JavaScript", async () => {
  // Same game, two widths: the numbers must move, and move the way the engine
  // says. This is the check that the page's figures are the engine's figures.
  const engine = await loadEngineFromBytes(wasmBytes());
  engine.genGame(42, 40, 7);
  const narrow = engine.solve(8, 1);
  const wide = engine.solve(8, 25);
  assert.equal(narrow.status, 0);
  assert.equal(wide.status, 0);
  assert.ok(wide.evaluations > narrow.evaluations, "a wider beam evaluates more placements");
});

test("a view is re-created across a memory growth", async () => {
  const engine = await loadEngineFromBytes(wasmBytes());
  engine.genGame(42, 40, 7);

  const stale = engine.board();
  assert.equal(stale.length, 10);

  const offset = engine.allocBytes(64 * 1024 * 1024);
  assert.ok(offset > ABI_END, `alloc returned 0x${offset.toString(16)}, above the §4 buffers`);

  assert.equal(stale.byteLength, 0, "the old view detaches; the loader must not cache it");
  const fresh = engine.board();
  assert.equal(fresh.byteLength, 40);
  assert.deepEqual([...fresh], SEED_42_BOARD, "and it still points at the same Board");
});

test("an out-of-Domain gen_game returns a typed code instead of trapping", async () => {
  const engine = await loadEngineFromBytes(wasmBytes());
  assert.equal(engine.genGame(0, 201, 7), 1, "pieces out of domain");
  assert.equal(engine.genGame(0, 40, 1), 1, "height cap out of domain");
  assert.equal(engine.genGame(0, 40, 7), 0, "and the instance still works afterwards");
});

test("solving before a game is generated is status 4, not a trap", async () => {
  const engine = await loadEngineFromBytes(wasmBytes());
  const outcome = engine.solve(8, 25);
  assert.equal(outcome.status, 4, "illegal ABI state");
  assert.equal(outcome.won, false);
});

test("no data segment overlaps the §4 ABI buffers", () => {
  const bytes = wasmBytes();
  const segments = dataSegments(bytes);
  assert.ok(segments.length > 0, "the module carries at least one data segment");
  for (const segment of segments) {
    if (segment.offset === null) continue; // passive: never mapped
    const start = segment.offset;
    const end = segment.offset + segment.length;
    assert.ok(
      end <= BOARD_OFF || start >= ABI_END,
      `data segment [0x${start.toString(16)}, 0x${end.toString(16)}) reaches into the ABI buffers below 0x460`,
    );
  }
});

test("the ABI buffers are zero at boot and writable in place", async () => {
  const bytes = wasmBytes();
  const { instance } = await WebAssembly.instantiate(bytes, {});
  const exports = instance.exports;

  const head = new Uint8Array(exports.memory.buffer, 0, ABI_END);
  const nonZero = [...head].filter((b) => b !== 0).length;
  assert.equal(nonZero, 0, "nothing may be written below 0x460 before the first call");

  // The canary: the board the engine writes must be readable at exactly 0x00,
  // which is the address where a plain (non-volatile) store silently vanished.
  assert.equal(exports.gen_game(42, 40, 7), 0);
  const words = new Uint32Array(exports.memory.buffer, BOARD_OFF, 10);
  assert.deepEqual([...words], SEED_42_BOARD);

  // And the solve writes its move list at exactly 0x140.
  const packed = BigInt.asUintN(64, exports.solve(8, 25));
  const moveCount = Number((packed >> 49n) & 0x1ffn);
  assert.ok(moveCount > 0 && moveCount <= 200, "the move count fits the moves buffer");
  const moves = new Uint32Array(exports.memory.buffer, MOVES_OFF, moveCount);
  assert.ok(moves[0] !== 0 || moves[1] !== 0, "the moves buffer received the move list");
});
