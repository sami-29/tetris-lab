// The seven tetrominoes as glyphs, for drawing only.
//
// The rows are §5.2's `_RAW_SHAPES` cells, transcribed the same way
// `engine/src/pieces.rs` transcribes them: one string per box row, top row first,
// one character per box column. Rotation *order* is contract — the ABI's move
// encoding names a rotation index — so the lists below keep §5.2's order.
//
// This table decides where a piece is *drawn*. It does not decide what a piece is:
// the engine's own table does that, and `web/test/playback.test.mjs` replays the
// artifact's Move list through these glyphs and checks the Lines it clears against
// the engine's count.

import type { Shape } from "./field.ts";

const SHAPES: ReadonlyArray<ReadonlyArray<ReadonlyArray<string>>> = [
  // I
  [["1111"], ["1", "1", "1", "1"]],
  // J
  [
    ["100", "111"],
    ["11", "10", "10"],
    ["111", "001"],
    ["01", "01", "11"],
  ],
  // L
  [
    ["001", "111"],
    ["10", "10", "11"],
    ["111", "100"],
    ["11", "01", "01"],
  ],
  // O
  [["11", "11"]],
  // S
  [
    ["011", "110"],
    ["10", "11", "01"],
  ],
  // T
  [
    ["010", "111"],
    ["10", "11", "10"],
    ["111", "010"],
    ["01", "11", "01"],
  ],
  // Z
  [
    ["110", "011"],
    ["01", "11", "10"],
  ],
];

/** §4.1's `piece_index` order: I J L O S T Z. */
export const PIECE_LETTERS = "IJLOSTZ";

function build(rows: ReadonlyArray<string>): Shape {
  const height = rows.length;
  const width = rows[0].length;
  const cols: number[] = [];
  for (let x = 0; x < width; x += 1) cols.push(0);
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      if (rows[y][x] === "1") cols[x] |= 1 << (height - 1 - y);
    }
  }
  return { width, height, cols };
}

/** Rotation lists in `piece_index` order; lengths 2, 4, 4, 1, 2, 4, 2. */
export const ROTATIONS: ReadonlyArray<ReadonlyArray<Shape>> = SHAPES.map((rotations) =>
  rotations.map(build),
);

/** The rotation a §4.1.1 move word names, or `undefined` past the table. */
export function rotationOf(pieceIndex: number, rotation: number): Shape | undefined {
  return ROTATIONS[pieceIndex]?.[rotation];
}

/** The Sequence letter for a `piece_index`, for the status line. */
export function letterOf(pieceIndex: number): string {
  return PIECE_LETTERS[pieceIndex] ?? "?";
}
