// The board renderer's own layer: what a collapse frame actually paints.
//
// `playback-animation.test.mjs` proves the *overlay* the animator produces; this
// file proves the canvas turns that overlay into motion rather than a flash. It
// runs `BoardView` against a recording 2D context, so the assertion is about the
// rectangles drawn — a cell at a fractional row, a band shorter than a cell — and
// not about a screenshot somebody has to squint at.

import { test } from "node:test";
import assert from "node:assert/strict";

import { BoardView, PALETTE } from "../src/ui/board.ts";
import { H, W, fieldFrom } from "../src/ui/field.ts";

/** A canvas-shaped object that records every rectangle drawn on it. */
function fakeCanvas(cssWidth = 400, cssHeight = 800) {
  const calls = [];
  const ctx = {
    fillStyle: "",
    strokeStyle: "",
    lineWidth: 1,
    globalAlpha: 1,
    setTransform() {},
    beginPath() {},
    moveTo() {},
    lineTo() {},
    stroke() {},
    fillRect(x, y, w, h) {
      calls.push({ op: "fill", x, y, w, h, style: this.fillStyle, alpha: this.globalAlpha });
    },
    strokeRect(x, y, w, h) {
      calls.push({ op: "stroke", x, y, w, h, style: this.strokeStyle });
    },
  };
  const canvas = {
    width: 0,
    height: 0,
    dataset: {},
    getContext: () => ctx,
    getBoundingClientRect: () => ({ width: cssWidth, height: cssHeight }),
  };
  return { canvas, calls };
}

function view(cssWidth = 400, cssHeight = 800) {
  const { canvas, calls } = fakeCanvas(cssWidth, cssHeight);
  const board = new BoardView(canvas);
  board.resize();
  // 400 × 800 over 10 × 20 cells: the cell is 40 px and the well starts at 0,0.
  assert.equal(canvas.dataset.cell, "40");
  assert.equal(canvas.dataset.originX, "0");
  assert.equal(canvas.dataset.originY, "0");
  return { board, calls };
}

/** The cell-sized fills of a colour, as board rows and columns. */
function cellsDrawn(calls, style, cell = 40) {
  return calls
    .filter((call) => call.op === "fill" && call.style === style && call.w === cell - 2)
    .map((call) => ({ col: (call.x - 1) / cell, row: (call.y - 1) / cell }));
}

test("a settled cell is drawn as one rect per set bit, at the bit's row", () => {
  const { board, calls } = view();
  const field = fieldFrom([0, 0, 0, 1 << 0, 0, 0, 0, 0, 0, 0]); // column 3, bit 0 = row 19
  board.draw(field);
  assert.deepEqual(cellsDrawn(calls, PALETTE.settled), [{ col: 3, row: 19 }]);
});

test("a collapse frame draws the moving cells at fractional rows", () => {
  const { board, calls } = view();
  board.draw(fieldFrom(new Uint32Array(W)), {
    cells: [
      { col: 3, row: 14.5 },
      { col: 8, row: 19 },
    ],
    bands: [{ row: 16, squeeze: 0.5 }],
  });

  const settled = cellsDrawn(calls, PALETTE.settled);
  assert.equal(settled.length, 2, "both moving cells are drawn");
  const frac = settled.find((cell) => cell.col === 3);
  assert.ok(frac, "the moving cell is on the canvas");
  assert.ok(Math.abs(frac.row - 14.5) < 1e-9, "at the row it has fallen to, not a snapped one");
  const still = settled.find((cell) => cell.col === 8);
  assert.ok(Math.abs(still.row - 19) < 1e-9, "the cell that does not move is drawn where it is");
});

test("a cleared row is a band contracting away, never a flash over the board", () => {
  const { board, calls } = view();
  board.draw(fieldFrom(new Uint32Array(W)), { bands: [{ row: 16, squeeze: 0.5 }] });

  const bands = calls.filter((call) => call.op === "fill" && call.style === PALETTE.clear);
  assert.equal(bands.length, 1, "one band for one cleared row");
  const band = bands[0];
  assert.equal(band.x, 0, "the band spans the well from its left edge");
  assert.equal(band.w, W * 40, "and the whole width");
  assert.equal(band.y, 16 * 40, "anchored at the cleared row's top edge");
  assert.equal(band.h, 20, "half a cell tall: the gap is opening downward");
  assert.ok(band.alpha < 1, "and fading as it goes");

  // The contrast: a flash would paint a full-height rect over the row. Nothing here
  // paints a cell-high rect at that row.
  assert.ok(
    !calls.some((call) => call.op === "fill" && call.y === 16 * 40 && call.h >= 40),
    "no full-height band anywhere",
  );
});

test("a landed piece keeps a contact edge for a beat", () => {
  const { board, calls } = view();
  const piece = [
    { col: 4, row: 18 },
    { col: 5, row: 18 },
    { col: 4, row: 19 },
    { col: 5, row: 19 },
  ];
  board.draw(fieldFrom(new Uint32Array(W)), { piece, impact: true });

  const inFlight = cellsDrawn(calls, PALETTE.inFlight);
  assert.equal(inFlight.length, 4, "the piece is drawn in flight colour while it lands");
  const contacts = calls.filter((call) => call.op === "fill" && call.style === PALETTE.impact && call.h === 2);
  assert.equal(contacts.length, 4, "each cell of the landed piece shows a contact edge");
  for (const contact of contacts) {
    assert.ok(contact.y > 18 * 40, "the edge sits under the piece's cells, at the contact");
  }
});

test("the ghost is an outline, and the palette is one replaceable object", () => {
  const { board, calls } = view();
  board.draw(fieldFrom(new Uint32Array(W)), { ghost: [{ col: 0, row: 19 }] });
  const outlines = calls.filter((call) => call.op === "stroke" && call.style === PALETTE.ghost);
  assert.equal(outlines.length, 1, "one outline per ghost cell");
  assert.equal(outlines[0].w, 39, "a cell-inset outline, not a filled cell");

  // Ticket 14 replaces the visual system from here and nowhere else.
  for (const value of Object.values(PALETTE)) {
    assert.match(value, /^#[0-9a-f]{6}$/, "every palette entry is a hex colour");
  }
  assert.equal(H, 20, "the board is twenty rows");
});
