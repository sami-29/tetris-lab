// Playback parity: the page replays the engine's Move list, so the replay must
// clear exactly the Lines the engine says it cleared.
//
// The ABI has no `set_board`, so the only way to draw move *k* is to replay moves
// 0..k-1 in the page (`web/src/ui/field.ts` + `pieces.ts`). That is a second
// implementation of gravity and line clearing, and a second implementation can
// drift. This test is the leash: the glyph table and the field must reproduce the
// engine's own board evolution, move for move, or the picture on the page is a
// fiction even when every number beside it is right.

import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { loadEngineFromBytes } from "../src/core/loader.ts";
import { rotationOf, PIECE_LETTERS } from "../src/ui/pieces.ts";
import { W, H, fieldFrom, restShift, place, clearFullRows, cellsAt, bitOf } from "../src/ui/field.ts";

const ARTIFACT = fileURLToPath(new URL("../pkg/engine.wasm", import.meta.url));

function wasmBytes() {
  if (!existsSync(ARTIFACT)) {
    throw new Error(`missing ${ARTIFACT} — run \`node scripts/build-wasm.mjs\` first`);
  }
  return new Uint8Array(readFileSync(ARTIFACT));
}

/** §4.1.1: `(piece_index << 16) | (rotation << 8) | column`. */
function unpackMove(word) {
  return { piece: (word >>> 16) & 0xff, rotation: (word >>> 8) & 0xff, column: word & 0xff };
}

/** Play the whole Move list on a JS field, as the page does. */
function replay(boardWords, moveWords) {
  const field = fieldFrom(boardWords);
  let lines = 0;
  const placements = [];
  for (const [index, word] of [...moveWords].entries()) {
    const move = unpackMove(word);
    const shape = rotationOf(move.piece, move.rotation);
    assert.ok(shape, `move ${index} names a rotation the table has`);
    const shift = restShift(field, shape, move.column);
    assert.notEqual(shift, null, `move ${index} (${JSON.stringify(move)}) is spawn-legal`);
    place(field, shape, move.column, shift);
    const cleared = clearFullRows(field);
    lines += cleared;
    placements.push({ ...move, shift, cleared });
  }
  return { field, lines, placements };
}

test("the glyph table is the seven pieces with §5.2's rotation counts and widths", () => {
  const widths = [
    [4, 1],
    [3, 2, 3, 2],
    [3, 2, 3, 2],
    [2],
    [3, 2],
    [3, 2, 3, 2],
    [3, 2],
  ];
  for (const [piece, letter] of [...PIECE_LETTERS].entries()) {
    const rotations = Array.from({ length: 4 }, (_, r) => rotationOf(piece, r)).filter(Boolean);
    assert.equal(rotations.length, widths[piece].length, `${letter} has §5.2's rotation count`);
    assert.deepEqual(
      rotations.map((shape) => shape.width),
      widths[piece],
      `${letter} widths`,
    );
    for (const shape of rotations) {
      assert.ok(shape.height >= 1 && shape.height <= 4, `${letter} height in a box`);
      const cells = cellsAt(shape, 0, 0);
      assert.equal(cells.length, 4, `${letter} rotation holds four cells`);
      for (const cell of cells) assert.ok(cell.bit >= 0 && cell.bit < H, `${letter} bit in the field`);
    }
  }
});

test("replaying the engine's Move list clears the engine's Lines", async () => {
  const engine = await loadEngineFromBytes(wasmBytes());

  // Three games, including the committed golden one, at the width the page uses.
  for (const [seed, goal] of [
    [0, 8],
    [42, 8],
    [7, 8],
  ]) {
    assert.equal(engine.genGame(seed, 40, 7), 0, `seed ${seed} generates`);
    const board = Uint32Array.from(engine.board());
    const outcome = engine.solve(goal, 25);
    assert.equal(outcome.status, 0, `seed ${seed} solves`);
    assert.ok(outcome.won, `seed ${seed} wins at width 25`);

    const moves = Uint32Array.from(engine.moves(outcome.moveCount));
    assert.equal(moves.length, outcome.moveCount);

    const { lines, placements } = replay(board, moves);
    assert.equal(lines, outcome.lines, `seed ${seed}: replayed Lines match the engine's`);
    assert.ok(
      placements.length > 0 && placements.length <= 40,
      `seed ${seed}: a move per piece at most`,
    );

    // The sequence's letters must agree with the move words' piece indices: the
    // page picks a glyph from the move, not from the Sequence.
    const sequence = engine.sequence(40);
    for (const [index, placement] of placements.entries()) {
      assert.equal(
        PIECE_LETTERS[placement.piece],
        sequence[index],
        `seed ${seed}: move ${index} names the Sequence's piece`,
      );
    }
  }
});

test("the field's bit layout is §5.1's", () => {
  // A move list replayed onto an empty field lands on the floor: the bottom row
  // is row 19, which is bit 0 of the column word.
  const field = fieldFrom([0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
  const o = rotationOf(3, 0); // O, 2×2
  const shift = restShift(field, o, 4);
  assert.equal(shift, 0, "an empty field rests the O on the floor");
  place(field, o, 4, shift);
  assert.equal(field[4] & 0b11, 0b11, "columns 4 and 5 have their bottom two bits set");
  assert.equal(field[5] & 0b11, 0b11);
  assert.equal(W, 10);
  assert.equal(bitOf(19), 0, "row 19 is bit 0");
  assert.equal(bitOf(0), H - 1, "row 0 is the top bit");
});
