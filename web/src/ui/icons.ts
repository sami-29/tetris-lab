// The icon set and the tetromino glyphs, by hand.
//
// §2.1.3: "Hand-made SVG icons and tetromino glyphs. No emoji, no stock dashboard
// furniture, no stock chart library look." So every mark on the page is a shape
// declared here — no icon font, no sprite sheet, no path data pasted from a set whose
// licence and stroke weight nobody in this repo chose.
//
// Two rules the set keeps, because a mismatched family is the thing that reads as
// "stock":
//
//   * **one stroke weight per purpose** — 1.2 px for outlines, 1.1 px for the
//     circular arrow, and *filled* only where the meaning is a filled shape (play,
//     pause, stop, the brand). Twelve- or eleven-pixel grid, drawn at 10–12 px.
//   * **`currentColor` everywhere**, so an icon inherits the ink ramp and the two
//     accents and no icon can introduce a third colour.
//
// The glyphs are the same rotation table the Board draws from (`ui/pieces.ts`), one
// `<rect>` per cell: a 3 px cell in the move strip, 4 px in the transport and the
// table, 6 px in "next four". The letter travels with the glyph as its `aria-label`,
// and it is the tooltip's first word (§2.1.2).

import { PIECE_LETTERS, ROTATIONS } from "./pieces.ts";

export type IconName =
  | "brand-mark"
  | "help"
  | "play"
  | "pause"
  | "step-back"
  | "step-forward"
  | "restart"
  | "close"
  | "expert-chevron"
  | "tree-toggle"
  | "stop";

const AMBER = "#e0a33e";
const TEAL = "#3d8f8a";
const DARK = "#2b3238";

/**
 * One icon as an SVG string, sized `size` CSS px.
 *
 * Strings rather than elements: the callers build rows of markup in one pass
 * (`innerHTML` on a container they own), which is fewer allocations than a dozen
 * `createElementNS` calls per row and no slower to read.
 */
export function svgIcon(name: IconName, size = 12): string {
  switch (name) {
    case "brand-mark":
      // The mockup's own mark: four squares, two amber and two teal, on an 18 grid.
      return `<svg class="ico" width="18" height="18" viewBox="0 0 18 18" aria-hidden="true"><rect x="1" y="1" width="7" height="7" fill="${AMBER}"/><rect x="10" y="1" width="7" height="7" fill="${TEAL}"/><rect x="1" y="10" width="7" height="7" fill="${TEAL}"/><rect x="10" y="10" width="7" height="7" fill="${DARK}"/></svg>`;
    case "help":
      return `<svg class="ico" width="${size}" height="${size}" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" aria-hidden="true"><circle cx="6" cy="6" r="4.6"/><path d="M6 5.4v3.1"/><circle cx="6" cy="3.3" r="0.65" fill="currentColor" stroke="none"/></svg>`;
    case "play":
      return `<svg class="ico" width="${size}" height="${size}" viewBox="0 0 10 10" fill="currentColor" aria-hidden="true"><path d="M1 0l8 5-8 5z"/></svg>`;
    case "pause":
      return `<svg class="ico" width="${size}" height="${size}" viewBox="0 0 10 10" fill="currentColor" aria-hidden="true"><rect x="1.4" y="0.6" width="2.7" height="8.8"/><rect x="5.9" y="0.6" width="2.7" height="8.8"/></svg>`;
    case "step-back":
      return `<svg class="ico" width="${size}" height="${size}" viewBox="0 0 10 10" fill="currentColor" aria-hidden="true"><rect x="1" y="0.6" width="1.9" height="8.8"/><path d="M9 0.6L3.6 5 9 9.4z"/></svg>`;
    case "step-forward":
      return `<svg class="ico" width="${size}" height="${size}" viewBox="0 0 10 10" fill="currentColor" aria-hidden="true"><rect x="7.1" y="0.6" width="1.9" height="8.8"/><path d="M1 0.6L6.4 5 1 9.4z"/></svg>`;
    case "restart":
      // A circular arrow: three quarters of a turn and an arrowhead, 1.1 stroke.
      return `<svg class="ico" width="${size}" height="${size}" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.1" stroke-linecap="round" aria-hidden="true"><path d="M10 6a4 4 0 1 1-1.6-3.2"/><path d="M10.2 1.4v2.2H8" stroke-linejoin="round"/></svg>`;
    case "close":
      return `<svg class="ico" width="${size}" height="${size}" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" aria-hidden="true"><path d="M2.6 2.6l6.8 6.8M9.4 2.6l-6.8 6.8"/></svg>`;
    case "expert-chevron":
      return `<svg class="ico chev" width="${size}" height="${size}" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" aria-hidden="true"><path d="M2.8 4.6L6 7.8l3.2-3.2"/></svg>`;
    case "tree-toggle":
      // A 12×12 glyph of the view it opens: three nodes and two edges.
      return `<svg class="ico" width="${size}" height="${size}" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" aria-hidden="true"><path d="M6 2.4v2.6M6 5v2.2M6 7.2L3 9.6M6 7.2l3 2.4"/><rect x="5" y="0.9" width="2" height="2"/><rect x="1.9" y="8.9" width="2" height="2"/><rect x="8.1" y="8.9" width="2" height="2"/></svg>`;
    case "stop":
      // A filled square, drawn: the filled-square character is not an icon.
      return `<svg class="ico" width="${size}" height="${size}" viewBox="0 0 10 10" fill="currentColor" aria-hidden="true"><rect x="1" y="1" width="8" height="8"/></svg>`;
    default: {
      const exhaustive: never = name;
      throw new Error(`no such icon: ${String(exhaustive)}`);
    }
  }
}

/**
 * One tetromino, drawn from the same rotation table the Board uses.
 *
 * `cell` is the cell size in CSS px: 3 in the move strip, 4 in the transport and the
 * per-layer table, 6 in "next four". The boxes are inset by 1 px so two touching
 * cells read as two cells.
 */
export function pieceGlyph(letter: string, cell = 4): string {
  const index = PIECE_LETTERS.indexOf(letter.toUpperCase());
  if (index < 0) return "";
  const shape = ROTATIONS[index][0];
  const width = shape.width * cell;
  const height = shape.height * cell;
  const cells: string[] = [];
  for (let x = 0; x < shape.width; x += 1) {
    for (let y = 0; y < shape.height; y += 1) {
      if ((shape.cols[x] & (1 << (shape.height - 1 - y))) === 0) continue;
      cells.push(`<rect x="${x * cell}" y="${y * cell}" width="${cell - 1}" height="${cell - 1}"/>`);
    }
  }
  return `<svg class="glyph" width="${width - 1}" height="${height - 1}" viewBox="0 0 ${width - 1} ${height - 1}" fill="currentColor" role="img" aria-label="${letter.toUpperCase()}" data-letter="${letter.toUpperCase()}">${cells.join("")}</svg>`;
}

/** The seven letters, for a legend or a size check. */
export const GLYPH_LETTERS = PIECE_LETTERS;
