// The Board on a Canvas 2D surface: ten columns by twenty rows, drawn from the
// column words the loader read out of wasm memory.
//
// Deliberately plain. The visual system arrives with ticket 14; what this ticket
// owes is a board that is *correct* — every filled cell is a bit, a cleared row is
// gone — and a drop that visibly lands. So: one accent for the settled stack, one
// for the piece in flight, an outline where it will land, and no decoration.
//
// Sizing comes from the CSS box, not from constants: the canvas fills its
// container and the cell size is the largest whole number of pixels that keeps
// 10:20 inside it, so the grid never blurs across a fractional device pixel.

import { H, W, type Field, type Shape, cellsAt } from "./field.ts";

export interface Overlay {
  /** Cells of the piece in flight, as board rows (0 = top), possibly fractional. */
  piece?: ReadonlyArray<{ col: number; row: number }>;
  /** Cells of where it will land, drawn as an outline. */
  ghost?: ReadonlyArray<{ col: number; row: number }>;
  /**
   * The settled cells of a clear in progress, at their interpolated rows. A clear
   * moves cells, so a renderer that carries them here is drawing the collapse; the
   * board underneath is empty while it does.
   */
  cells?: ReadonlyArray<{ col: number; row: number }>;
  /**
   * The cleared rows, contracting away: `squeeze` is the fraction of a cell height
   * the band still occupies, anchored at the band's top edge so the gap opens
   * downward. A band is never a translucent flash over the finished board.
   */
  bands?: ReadonlyArray<{ row: number; squeeze: number }>;
  /** The piece has just touched down: draw it with a bright contact edge. */
  impact?: boolean;
  /** Board rows that are full right now. Kept for a caller that wants a plain flash. */
  clearRows?: readonly number[];
}

/**
 * The palette, in one place.
 *
 * Ticket 14 replaces this shell and its chrome; when it does, this object is the
 * whole colour system the board draws from, so the change is one file and not a
 * search for hex literals.
 */
export const PALETTE = {
  background: "#0b0f14",
  grid: "#1a2430",
  settled: "#7f9dc0",
  settledEdge: "#a8c2e0",
  /** The piece in flight. */
  inFlight: "#e8f0ff",
  ghost: "#3a4d63",
  /** A cleared row's band, contracting. */
  clear: "#d7e4f5",
  /** The contact edge of a piece that has just landed. */
  impact: "#ffffff",
} as const;

const { background: BACKGROUND, grid: GRID } = PALETTE;
const SETTLED = PALETTE.settled;
const SETTLED_EDGE = PALETTE.settledEdge;
const IN_FLIGHT = PALETTE.inFlight;
const GHOST = PALETTE.ghost;
const CLEAR = PALETTE.clear;

export class BoardView {
  private readonly canvas: HTMLCanvasElement;
  private readonly ctx: CanvasRenderingContext2D;
  private cell = 16;
  private originX = 0;
  private originY = 0;

  constructor(canvas: HTMLCanvasElement) {
    const ctx = canvas.getContext("2d");
    if (!ctx) throw new Error("this browser has no Canvas 2D context");
    this.canvas = canvas;
    this.ctx = ctx;
  }

  /** Match the backing store to the CSS box and the device pixel ratio. */
  resize(): void {
    const rect = this.canvas.getBoundingClientRect();
    const dpr = globalThis.devicePixelRatio || 1;
    const cssWidth = Math.max(1, Math.floor(rect.width));
    const cssHeight = Math.max(1, Math.floor(rect.height));
    this.canvas.width = Math.floor(cssWidth * dpr);
    this.canvas.height = Math.floor(cssHeight * dpr);
    this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

    // The largest whole cell that fits, so cell edges land on device pixels.
    this.cell = Math.max(4, Math.floor(Math.min(cssWidth / W, cssHeight / H)));
    this.originX = Math.floor((cssWidth - this.cell * W) / 2);
    this.originY = Math.floor((cssHeight - this.cell * H) / 2);

    // The geometry a headless check needs to read a cell back out of the pixels
    // (`web/scripts/browser-check.mjs`); in CSS pixels, so multiply by the DPR.
    this.canvas.dataset.cell = String(this.cell);
    this.canvas.dataset.originX = String(this.originX);
    this.canvas.dataset.originY = String(this.originY);
    this.canvas.dataset.rows = String(H);
    this.canvas.dataset.cols = String(W);
  }

  draw(field: Field, overlay: Overlay = {}): void {
    const { ctx, cell } = this;
    const cssWidth = this.canvas.width / (globalThis.devicePixelRatio || 1);
    const cssHeight = this.canvas.height / (globalThis.devicePixelRatio || 1);

    ctx.fillStyle = BACKGROUND;
    ctx.fillRect(0, 0, cssWidth, cssHeight);

    // The well itself, so an empty board still reads as a board.
    ctx.fillStyle = BACKGROUND;
    ctx.fillRect(this.originX, this.originY, cell * W, cell * H);

    ctx.strokeStyle = GRID;
    ctx.lineWidth = 1;
    ctx.beginPath();
    for (let c = 1; c < W; c += 1) {
      const x = this.originX + c * cell + 0.5;
      ctx.moveTo(x, this.originY);
      ctx.lineTo(x, this.originY + cell * H);
    }
    for (let r = 1; r < H; r += 1) {
      const y = this.originY + r * cell + 0.5;
      ctx.moveTo(this.originX, y);
      ctx.lineTo(this.originX + cell * W, y);
    }
    ctx.stroke();

    // The settled stack: one rect per set bit.
    for (let c = 0; c < W; c += 1) {
      for (let bit = 0; bit < H; bit += 1) {
        if ((field[c] & (1 << bit)) !== 0) this.cellRect(c, H - 1 - bit, SETTLED, SETTLED_EDGE);
      }
    }

    for (const row of overlay.clearRows ?? []) {
      ctx.fillStyle = CLEAR;
      ctx.globalAlpha = 0.55;
      ctx.fillRect(this.originX, this.originY + row * cell, cell * W, cell);
      ctx.globalAlpha = 1;
    }

    // The settled cells of a collapse: the same cell rects as the stack, at rows
    // that may be fractional, so the segments visibly fall rather than jump.
    for (const { col, row } of overlay.cells ?? []) {
      this.cellRect(col, row, SETTLED, SETTLED_EDGE);
    }

    // The cleared bands, contracting from their top edge: draw them after the
    // moving cells so a cell never shows through a gap it has already passed.
    for (const band of overlay.bands ?? []) {
      const height = Math.max(0, Math.min(1, band.squeeze)) * cell;
      if (height <= 0.5) continue;
      ctx.fillStyle = CLEAR;
      ctx.globalAlpha = Math.min(1, 0.35 + band.squeeze);
      ctx.fillRect(this.originX, this.originY + band.row * cell, cell * W, height);
      ctx.globalAlpha = 1;
    }

    for (const { col, row } of overlay.ghost ?? []) {
      ctx.strokeStyle = GHOST;
      ctx.lineWidth = 1;
      ctx.strokeRect(this.originX + col * cell + 0.5, this.originY + row * cell + 0.5, cell - 1, cell - 1);
    }

    for (const { col, row } of overlay.piece ?? []) {
      this.cellRect(col, row, IN_FLIGHT, overlay.impact ? PALETTE.impact : "#ffffff");
    }

    // A landed piece leaves a contact edge under its lowest cells for a beat: the
    // difference between "it arrived" and "it appeared at the bottom".
    if (overlay.impact && (overlay.piece?.length ?? 0) > 0) {
      ctx.fillStyle = PALETTE.impact;
      for (const { col, row } of overlay.piece!) {
        ctx.fillRect(this.originX + col * cell + 1, this.originY + (row + 1) * cell - 2, cell - 2, 2);
      }
    }

    // The well's edge last, so nothing painted over it.
    ctx.strokeStyle = GRID;
    ctx.lineWidth = 2;
    ctx.strokeRect(this.originX - 1, this.originY - 1, cell * W + 2, cell * H + 2);
  }

  /** A filled cell at fractional coordinates, in board rows. */
  private cellRect(col: number, row: number, fill: string, edge: string): void {
    const { ctx, cell } = this;
    const x = this.originX + col * cell;
    const y = this.originY + row * cell;
    ctx.fillStyle = fill;
    ctx.fillRect(x + 1, y + 1, cell - 2, cell - 2);
    ctx.fillStyle = edge;
    ctx.fillRect(x + 1, y + 1, cell - 2, Math.max(1, Math.round(cell * 0.12)));
  }

  /** The cells a rotation would occupy resting at `shift`, as board rows. */
  static cells(shape: Shape, column: number, shift: number): Array<{ col: number; row: number }> {
    return cellsAt(shape, column, shift).map(({ col, bit }) => ({ col, row: H - 1 - bit }));
  }
}
