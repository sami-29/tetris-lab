// The centre readout's arithmetic: the column profile, in rows and holes.
//
// §2's centre carries "a readout in words (what the evaluator is charging the
// position for, with the score), the per-column profile, the current placement, the
// next four pieces, this layer's counts, and the score across layers as a sparkline".
// This module is the second of those; the rest are written straight from the trace and
// the replay in `main.ts`.
//
// The profile is a *replay* figure, not an engine one: it is the Board at the Move on
// screen, which is `board0` plus the Move list `buildTimeline`/`reconstruct` already
// computes. Nothing here scores anything, and nothing here invents a Feature: the
// per-Feature breakdown the mockup drew cannot be computed in the page at all, because
// the ABI reports a state's total score and not its terms (notes/14 Q1).
//
// Vocabulary, from `GLOSSARY.md`: a **Board** is ten column words, bit *n* of a column
// is row 19−*n*, so bit 0 is the floor. "Height" is therefore "how many rows of this
// column are occupied, counted up from the floor", and a **hole** is an empty cell with
// a filled cell above it in the same column — which is exactly the empty bits below the
// column's highest set bit.

import { H, W } from "./field.ts";

export interface ColumnEntry {
  /** 0–9, left to right. */
  readonly column: number;
  /** Rows occupied, floor-anchored: the top of the stack. */
  readonly height: number;
  /** Empty cells under a filled one. */
  readonly holes: number;
}

/** Ten columns of height and holes, from one Board. */
export function columnProfile(field: ArrayLike<number>): ColumnEntry[] {
  const profile: ColumnEntry[] = [];
  for (let column = 0; column < W; column += 1) {
    const word = field[column] ?? 0;
    // The highest set bit is the top of the stack; bit 0 is row 19, the floor.
    const top = word === 0 ? -1 : 31 - Math.clz32(word);
    let holes = 0;
    for (let bit = 0; bit < top; bit += 1) {
      if ((word & (1 << bit)) === 0) holes += 1;
    }
    profile.push({ column, height: top + 1, holes });
  }
  return profile;
}

/** One column, in words, for the bar's tooltip: `column 7 · height 7 rows · 4 holes`. */
export function profileTip(column: number, entry: ColumnEntry): string {
  const rows = entry.height === 1 ? "row" : "rows";
  const holes = entry.holes === 1 ? "hole" : "holes";
  return `column ${column} · height ${entry.height} ${rows} · ${entry.holes} ${holes} · of ${H} board rows`;
}

/** The tallest column on screen: the scale a bar is drawn against, so bars compare. */
export function profileScale(profile: readonly ColumnEntry[]): number {
  return Math.max(1, ...profile.map((entry) => entry.height));
}
