// The playback timeline: the Board the engine produced, the Sequence's Move list,
// and the position after every Move.
//
// There is no `set_board` and no per-Move board export in the ABI (§4), and `solve`
// leaves the board buffer untouched, so the page's only way to draw Move *k* is to
// replay Moves 1..k locally. That makes this file a second implementation of
// gravity, placement and clearing — so it is on a leash:
// `web/test/timeline.test.mjs` compares it against
// `web/test/fixtures/replay-vectors.json`, which `web/scripts/gen-replay-vectors`
// writes by linking **the engine crate itself**. Word for word, Move for Move.
//
// Two properties are load-bearing for the rest of the ticket:
//
//   * `reconstruct` is pure and hands back a **fresh** array every time, so a
//     renderer cannot write history and a seek cannot see a stale position;
//   * the timeline carries **no time at all**. When a Move is drawn and for how
//     long is `phases.ts` and `clock.ts`'s business, which is what lets a seek be
//     "draw `reconstruct(k)` on the next frame" with nothing in flight to cancel.

import {
  H,
  W,
  cellsAt,
  clearFullRows,
  fieldFrom,
  fullRows,
  place,
  restShift,
  type Field,
  type Shape,
} from "./field.ts";
import { rotationOf } from "./pieces.ts";

/** §4.1.1's Move, decoded. */
export interface MoveWord {
  piece: number;
  rotation: number;
  column: number;
}

/** A cell of the well, as board coordinates: row 0 is the top row. */
export interface Cell {
  col: number;
  row: number;
}

/** One surviving row's journey: it starts at `from` and lands at `to`. */
export interface RowMove {
  from: number;
  to: number;
}

/**
 * A run of consecutive surviving rows that all fall by the same distance.
 *
 * A clear is **not** necessarily contiguous — a piece can complete rows 14 and 17
 * while leaving 15 and 16 unfilled — so a clear is one to five of these, each with
 * its own fall distance. Drawing "the stack above the clear" as one band is wrong.
 */
export interface Segment {
  from: number;
  to: number;
  rows: number;
  fall: number;
}

export interface CollapsePlan {
  /** Every surviving row, top first: `from` is where it is, `to` is where it goes. */
  moves: RowMove[];
  /** The rows that disappear, ascending (board rows, 0 = top). */
  vanish: number[];
  /** The run structure the renderer animates, top-down. */
  segments: Segment[];
}

/** One Move, and everything that happened to the Board because of it. */
export interface Step {
  /** 1-based Move index. */
  k: number;
  piece: number;
  rotation: number;
  column: number;
  /** §5.3's spawn: the box top at row 0. */
  spawnShift: number;
  /** The engine's `rest_shift` — where the piece actually came to rest. */
  restShift: number;
  /** How far the piece fell, `spawnShift - restShift`. */
  dropRows: number;
  spawnCells: Cell[];
  restCells: Cell[];
  /** The board this Move is applied to (the board after Move k-1). */
  preBoard: Field;
  /** preBoard with the piece stamped at rest: the placement, before any clear. */
  preClear: Field;
  /** preClear with the cleared rows removed: the board after Move k. */
  postClear: Field;
  /** The rows this Move completed, ascending. */
  cleared: number[];
  plan: CollapsePlan;
  linesBefore: number;
  linesAfter: number;
}

export interface Timeline {
  /** The Board the engine produced, as a copy. */
  board0: Field;
  moves: MoveWord[];
  /** `steps[k - 1]` is Move k. */
  steps: Step[];
  /** Cumulative Lines cleared by the whole Move list (§4.2's `lines`). */
  lines: number;
}

/** §4.1.1: `(piece_index << 16) | (rotation << 8) | column`. */
export function unpackMove(word: number): MoveWord {
  return {
    piece: (word >>> 16) & 0xff,
    rotation: (word >>> 8) & 0xff,
    column: word & 0xff,
  };
}

function asMove(entry: number | MoveWord): MoveWord {
  if (typeof entry === "number") return unpackMove(entry);
  return { piece: entry.piece, rotation: entry.rotation, column: entry.column };
}

/**
 * How the surviving rows close over the cleared ones.
 *
 * A row's fall is the number of cleared rows *below* it (a larger board row), so
 * consecutive surviving rows share a distance exactly when nothing was cleared
 * between them. That is what makes the segments 1–5 runs rather than one band.
 */
export function collapsePlan(cleared: readonly number[]): CollapsePlan {
  const vanish = [...cleared].sort((a, b) => a - b);
  const moves: RowMove[] = [];
  const segments: Segment[] = [];
  let run: { from: number; to: number; fall: number } | null = null;
  const push = (): void => {
    if (!run) return;
    segments.push({ from: run.from, to: run.to, rows: run.to - run.from + 1, fall: run.fall });
    run = null;
  };

  for (let row = 0; row < H; row += 1) {
    if (vanish.includes(row)) {
      push();
      continue;
    }
    const fall = vanish.filter((clearedRow) => clearedRow > row).length;
    moves.push({ from: row, to: row + fall });
    if (run && run.fall === fall) run.to = row;
    else {
      push();
      run = { from: row, to: row, fall };
    }
  }
  push();
  return { moves, vanish, segments };
}

/**
 * Apply a plan to the board the piece landed on: every surviving row is copied to
 * where it fell to. The renderer animates by interpolating this same map, so the
 * plan and the board it produces cannot disagree — and `timeline.test.mjs` asserts
 * that this equals `clearFullRows` on arbitrary boards.
 */
export function applyPlan(preClear: Field, plan: CollapsePlan): Field {
  const out = fieldFrom(new Uint32Array(W));
  for (const move of plan.moves) {
    const source = H - 1 - move.from;
    const target = H - 1 - move.to;
    for (let col = 0; col < W; col += 1) {
      if ((preClear[col] >>> source) & 1) out[col] = (out[col] | (1 << target)) >>> 0;
    }
  }
  return out;
}

/** The engine's board for every Move: the one thing a seek and a play both read. */
export function buildTimeline(
  board0: ArrayLike<number>,
  moves: ReadonlyArray<number | MoveWord>,
): Timeline {
  const decoded = moves.map(asMove);
  const steps: Step[] = [];
  let field = fieldFrom(board0);
  let lines = 0;

  for (const [index, move] of decoded.entries()) {
    const k = index + 1;
    const shape: Shape | undefined = rotationOf(move.piece, move.rotation);
    if (!shape) {
      throw new Error(`move ${k}: piece ${move.piece} rotation ${move.rotation} is not a tetromino rotation`);
    }
    const rest = restShift(field, shape, move.column);
    if (rest === null) {
      // The Move list is the engine's, so this is a bug in the replay or a stale
      // artifact — never something to draw silently.
      throw new Error(
        `move ${k}: piece ${move.piece} rotation ${move.rotation} at column ${move.column} is not spawn-legal`,
      );
    }

    const preBoard = fieldFrom(field);
    const preClear = fieldFrom(field);
    place(preClear, shape, move.column, rest);

    const full = fullRows(preClear);
    // Top row first (board rows ascend), which is the order the strip and the
    // collapse plan both read.
    const cleared: number[] = [];
    for (let bit = H - 1; bit >= 0; bit -= 1) if ((full & (1 << bit)) !== 0) cleared.push(H - 1 - bit);

    const postClear = fieldFrom(preClear);
    const clearedLines = clearFullRows(postClear);
    if (clearedLines !== cleared.length) {
      throw new Error(`move ${k}: cleared ${clearedLines} rows but found ${cleared.length}`);
    }

    const spawnShift = H - shape.height;
    steps.push({
      k,
      piece: move.piece,
      rotation: move.rotation,
      column: move.column,
      spawnShift,
      restShift: rest,
      dropRows: spawnShift - rest,
      spawnCells: cellsAt(shape, move.column, spawnShift).map((cell) => ({ col: cell.col, row: H - 1 - cell.bit })),
      restCells: cellsAt(shape, move.column, rest).map((cell) => ({ col: cell.col, row: H - 1 - cell.bit })),
      preBoard,
      preClear,
      postClear,
      cleared,
      plan: collapsePlan(cleared),
      linesBefore: lines,
      linesAfter: lines + clearedLines,
    });

    lines += clearedLines;
    field = fieldFrom(postClear);
  }

  return { board0: fieldFrom(board0), moves: decoded, steps, lines };
}

/**
 * The Board after `k` Moves, `k` in `[0, n]`; out-of-range values clamp.
 *
 * Always a fresh array: no caller can write history through it, and no seek can
 * hand back a board some renderer painted into.
 */
export function reconstruct(timeline: Timeline, k: number): Field {
  const total = timeline.steps.length;
  const at = Number.isFinite(k) ? Math.max(0, Math.min(total, Math.trunc(k))) : 0;
  if (at === 0) return fieldFrom(timeline.board0);
  return fieldFrom(timeline.steps[at - 1].postClear);
}
