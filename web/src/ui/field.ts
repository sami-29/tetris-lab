// Playback geometry: the Board as ten column words, plus the drop and line-clear
// primitives a placement needs so a Move can be animated.
//
// This is **not** a second solver. Nothing on the page's status line comes from
// here — the Lines cleared, the Evaluations and won/lost are the engine's, handed
// over by the loader. What this file exists for is that the ABI has no `set_board`
// export (§4, open question Q3), so the only way to draw move *k* is to replay
// moves 0..k-1 locally. `web/test/playback.test.mjs` replays the artifact's own
// Move list through this file and asserts the Lines it clears match the engine's,
// which is what keeps the picture honest.
//
// The bit layout is §5.1, the same as `engine/src/board.rs`: bit *n* of column *c*
// is row `H-1-n`, so bit 0 is the bottom row and row 0 is the top row.

export const W = 10;
export const H = 20;

/** Ten `u32` column words (§4.1's board buffer, as the loader hands it over). */
export type Field = Uint32Array;

/** A rotation as `engine/src/pieces.rs` defines it: a bit per row per column. */
export interface Shape {
  readonly width: number;
  readonly height: number;
  readonly cols: readonly number[];
}

export function emptyField(): Field {
  return new Uint32Array(W);
}

/** A mutable copy of the Board the loader read out of wasm memory. */
export function fieldFrom(words: ArrayLike<number>): Field {
  const field = new Uint32Array(W);
  for (let c = 0; c < W; c += 1) field[c] = (words[c] ?? 0) >>> 0;
  return field;
}

/** Row 0 is the top row; `row` counts down. `bit` is the column word's bit index. */
export function bitOf(row: number): number {
  return H - 1 - row;
}

export function rowOf(bit: number): number {
  return H - 1 - bit;
}

export function filled(field: Field, col: number, row: number): boolean {
  return (field[col] & (1 << bitOf(row))) !== 0;
}

/** Stack height of one column: zero when empty, `H` when full. */
export function height(field: Field, col: number): number {
  return 32 - Math.clz32(field[col]);
}

/** Rows where every column is filled, as a mask over bit positions. */
export function fullRows(field: Field): number {
  let mask = field[0];
  for (let c = 1; c < W; c += 1) mask &= field[c];
  return mask >>> 0;
}

function popcount(mask: number): number {
  let value = mask;
  let count = 0;
  while (value !== 0) {
    value &= value - 1;
    count += 1;
  }
  return count;
}

/** Clear every full row, bottom-up, shifting the stack above each one down. */
export function clearFullRows(field: Field): number {
  const full = fullRows(field);
  if (full === 0) return 0;
  for (let c = 0; c < W; c += 1) {
    let word = field[c];
    for (let bit = H - 1; bit >= 0; bit -= 1) {
      if ((full & (1 << bit)) !== 0) {
        // Everything below the row stays; everything above it drops one place.
        const lower = word & ((1 << bit) - 1);
        const upper = (word >>> (bit + 1)) << bit;
        word = (lower | upper) >>> 0;
      }
    }
    field[c] = word;
  }
  return popcount(full);
}

/** `u8::trailing_zeros`, which is 8 for a zero column of the shape. */
function trailingZeros(byte: number): number {
  return byte === 0 ? 8 : 31 - Math.clz32(byte & -byte);
}

/**
 * How far a rotation must be shifted up from the floor to rest at `column`, or
 * `null` when it cannot enter at row 0 or does not fit horizontally. The same
 * rule as `Board::rest_shift` — that is why the replay agrees with the engine.
 */
export function restShift(field: Field, shape: Shape, column: number): number | null {
  if (column < 0 || column + shape.width > W) return null;
  let shift = 0;
  for (let i = 0; i < shape.width; i += 1) {
    const lowest = trailingZeros(shape.cols[i]);
    const needed = height(field, column + i) - lowest;
    if (needed > shift) shift = needed;
  }
  if (shift > H - shape.height) return null;
  return shift;
}

export function place(field: Field, shape: Shape, column: number, shift: number): void {
  for (let i = 0; i < shape.width; i += 1) {
    field[column + i] = (field[column + i] | (shape.cols[i] << shift)) >>> 0;
  }
}

/** Every filled cell of a rotation resting at `shift`, as (column, bit) pairs. */
export function cellsAt(shape: Shape, column: number, shift: number): Array<{ col: number; bit: number }> {
  const cells: Array<{ col: number; bit: number }> = [];
  for (let i = 0; i < shape.width; i += 1) {
    for (let offset = 0; offset < shape.height; offset += 1) {
      if ((shape.cols[i] & (1 << offset)) !== 0) {
        cells.push({ col: column + i, bit: shift + offset });
      }
    }
  }
  return cells;
}

/**
 * The landing preview for the drop animation: the resting shift a shape would
 * reach if it fell down `column` now.
 */
export function landingShift(field: Field, shape: Shape, column: number): number | null {
  return restShift(field, shape, column);
}
