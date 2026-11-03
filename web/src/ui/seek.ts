// The move strip is the seek bar: one gesture to any Move.
//
// The strip is a row of per-Move cells, and an `<input type="range" min=0 max=n`
// is laid over it invisibly, so a drag, a click, the arrow keys, Home/End and a
// screen reader all work for free. These two functions are the only arithmetic in
// that arrangement, kept here so they can be tested without a DOM:
//
//   * `xToMove` turns a pointer position into a Move index, with both ends exact —
//     `k = 0` is the starting Board, which the prototype's `clamp(1, TOTAL)` made
//     unreachable;
//   * `moveToX` is its inverse, for placing the playhead marker.
//
// `stripMode` decides whether a Move can be drawn as a glyph or only as a tick: at
// 200 Moves in a 640 px column a per-cell glyph has ~2 px, which is a tick mark, not
// a tetromino.

/** Past this many Moves the strip switches from glyphs to ticks. */
export const GLYPH_LIMIT = 60;

export type StripMode = "glyph" | "tick";

export function stripMode(moves: number): StripMode {
  return moves <= GLYPH_LIMIT ? "glyph" : "tick";
}

/**
 * The Move under `x`, as `0..moves` where `moves` is the length of the Move list.
 *
 * Out-of-range input clamps rather than wrapping: dragging past either end parks
 * the playhead at that end.
 */
export function xToMove(x: number, width: number, moves: number): number {
  const total = Math.max(0, Math.trunc(moves) || 0);
  if (!(width > 0) || total === 0) return 0;
  const ratio = Math.min(1, Math.max(0, (Number.isFinite(x) ? x : 0) / width));
  return Math.min(total, Math.max(0, Math.round(ratio * total)));
}

/** Where the playhead for Move `k` sits along a strip `width` wide. */
export function moveToX(k: number, width: number, moves: number): number {
  const total = Math.max(0, Math.trunc(moves) || 0);
  if (!(width > 0) || total === 0) return 0;
  const at = Math.min(total, Math.max(0, Number.isFinite(k) ? Math.trunc(k) : 0));
  return (at / total) * width;
}
