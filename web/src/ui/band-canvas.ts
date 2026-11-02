// The band, drawn. One `canvas`, one render function, no state of its own.
//
// This file is deliberately thin: every position it draws comes from
// `band-model.ts`, so the picture cannot disagree with the numbers the inspector
// prints, and the whole visual decision lives in `BAND_PALETTE` below — ticket 14
// owns the workbench's visual system and replaces that block; nothing else here
// needs to move.
//
// What it draws, bottom of the stack to the top:
//
//   * the axis: five labelled gridlines with the score numbers in the left gutter
//     (the scores are negative — they are the Evaluator's charge, best to worst);
//   * per row: a faint envelope from the row's worst score to its best, one mark
//     per *distinct* score (width by the gap to the next mark, alpha by how many
//     states share it), a 1-px median tick, the layer's index and piece letter in
//     the gutter and the kept count at the right edge;
//   * the winning lineage on top: an amber polyline with a dot per row, drawn from
//     its own records at its own scores. A clipped segment (a parent the band did
//     not keep) is drawn dashed, never as a straight line to the row's edge —
//     "this lineage has a hole" is information, a lie is not.
//
// Rows are *rows* (one per Layer) rather than the columns the prototype used: at
// the 100-piece configuration a ≥101-step game would give a column 5.4 px in an
// inspector, and a row the full width.

import type { BandModel, BandRow, LineagePoint } from "./band-model.ts";

/**
 * The band's whole colour system, in one place.
 *
 * Ticket 14 replaces this object with the workbench's tokens; the drawing code
 * reads nothing else. The score scale is three teals (the prototype's, whose amber
 * top tier is *dropped* so amber means "the lineage" and nothing else) and the
 * lineage is that amber.
 */
export const BAND_PALETTE = {
  background: "#0b0f14",
  panel: "#121a23",
  line: "#1a2430",
  grid: "rgba(44, 50, 56, 0.8)",
  ink: "#dbe6f2",
  muted: "#8ea3c0",
  /** Score tiers, worst third → best. Alpha is raised per mark by its state count. */
  score: ["rgba(61, 143, 138, 0.72)", "rgba(95, 192, 184, 0.92)", "rgba(140, 224, 214, 0.95)"] as const,
  /** The per-row envelope. */
  envelope: "rgba(61, 143, 138, 0.20)",
  median: "rgba(95, 192, 184, 0.9)",
  /** The winning lineage: a colour nothing else in the band uses. */
  lineage: "rgba(224, 163, 62, 0.95)",
  lineageEdge: "#0b0f14",
} as const;

/** Pixels of left gutter for the score axis and the layer ruler. */
const GUTTER_LEFT = 44;
/** Pixels of right gutter for a row's kept count. */
const GUTTER_RIGHT = 52;
const PADDING_Y = 6;

interface Geometry {
  /** CSS pixels of the plot area. */
  plotWidth: number;
  plotHeight: number;
  rowHeight: number;
  cellHeight: number;
}

export class BandView {
  private readonly canvas: HTMLCanvasElement;
  private readonly ctx: CanvasRenderingContext2D;

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
    this.canvas.dataset.width = String(cssWidth);
    this.canvas.dataset.height = String(cssHeight);
  }

  /** Draw the model, or nothing at all in the panel's colour. */
  draw(model: BandModel | null): void {
    const rect = this.canvas.getBoundingClientRect();
    const width = Math.max(1, Math.floor(rect.width));
    const height = Math.max(1, Math.floor(rect.height));
    const ctx = this.ctx;
    ctx.fillStyle = BAND_PALETTE.background;
    ctx.fillRect(0, 0, width, height);
    if (model === null || model.rows.length === 0) {
      this.canvas.dataset.rows = "0";
      return;
    }

    const geometry = this.geometry(width, height, model.rows.length);
    const { plotWidth, plotHeight } = geometry;
    const originX = GUTTER_LEFT;
    const originY = PADDING_Y;
    const xOf = (u: number): number => originX + Math.min(1, Math.max(0, u)) * plotWidth;

    this.drawAxis(model, geometry, xOf, originY);

    ctx.save();
    ctx.beginPath();
    ctx.rect(originX, originY, plotWidth, plotHeight);
    ctx.clip();
    for (const row of model.rows) this.drawRow(model, row, geometry, xOf, originY);
    ctx.restore();

    this.drawLineage(model, geometry, xOf, originY);

    // Geometry a headless check can read back out of the pixels (`browser-check.mjs`).
    this.canvas.dataset.rows = String(model.rows.length);
    this.canvas.dataset.band = String(model.band);
    this.canvas.dataset.records = String(model.records);
    this.canvas.dataset.rowHeight = String(geometry.rowHeight);
    this.canvas.dataset.originX = String(originX);
    this.canvas.dataset.originY = String(originY);
    this.canvas.dataset.plotWidth = String(plotWidth);
    this.canvas.dataset.lineage = model.lineage.map((point) => `${point.layer}:${point.rank}`).join(",");
  }

  private geometry(width: number, height: number, rows: number): Geometry {
    const plotWidth = Math.max(1, width - GUTTER_LEFT - GUTTER_RIGHT);
    const plotHeight = Math.max(1, height - PADDING_Y * 2);
    const rowHeight = plotHeight / rows;
    return { plotWidth, plotHeight, rowHeight, cellHeight: Math.max(1, rowHeight - 2) };
  }

  private drawAxis(
    model: BandModel,
    geometry: Geometry,
    xOf: (u: number) => number,
    originY: number,
  ): void {
    const ctx = this.ctx;
    ctx.save();
    ctx.font = "10px ui-monospace, monospace";
    ctx.textAlign = "right";
    ctx.textBaseline = "middle";
    for (const score of model.axis.ticks) {
      const x = xOf((model.axis.max - score) / (model.axis.max - model.axis.min || 1));
      ctx.strokeStyle = BAND_PALETTE.grid;
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(x, originY);
      ctx.lineTo(x, originY + geometry.plotHeight);
      ctx.stroke();
      ctx.fillStyle = BAND_PALETTE.muted;
      ctx.fillText(String(score), GUTTER_LEFT - 4, originY + geometry.plotHeight / 2);
    }
    ctx.restore();
  }

  private drawRow(
    model: BandModel,
    row: BandRow,
    geometry: Geometry,
    xOf: (u: number) => number,
    originY: number,
  ): void {
    const ctx = this.ctx;
    const top = originY + row.layer * geometry.rowHeight + 1;
    const bottom = top + geometry.cellHeight;
    if (row.marks.length === 0) return;

    // The envelope: the row's worst score to its best.
    const left = xOf(row.marks[row.marks.length - 1].u);
    const right = xOf(row.marks[0].u);
    ctx.fillStyle = BAND_PALETTE.envelope;
    ctx.fillRect(Math.min(left, right), top, Math.max(1, Math.abs(right - left)), geometry.cellHeight);

    // One mark per distinct score: width by the gap to the next mark, alpha by the
    // number of states that share the score.
    const widest = Math.max(...row.marks.map((mark) => mark.count), 1);
    const bestThird = model.axis.max - (model.axis.max - model.axis.min) / 3;
    const middleThird = model.axis.max - (2 * (model.axis.max - model.axis.min)) / 3;
    for (const mark of row.marks) {
      const x = xOf(mark.u);
      const tier = mark.score >= bestThird ? 2 : mark.score >= middleThird ? 1 : 0;
      ctx.globalAlpha = 0.3 + 0.7 * (mark.count / widest);
      ctx.fillStyle = BAND_PALETTE.score[tier];
      const markWidth = Math.max(1.5, geometry.plotWidth / Math.max(24, row.marks.length * 2));
      ctx.fillRect(x - markWidth / 2, top, markWidth, geometry.cellHeight);
    }
    ctx.globalAlpha = 1;

    // The median tick.
    const medianX = xOf((model.axis.max - row.median) / (model.axis.max - model.axis.min || 1));
    ctx.strokeStyle = BAND_PALETTE.median;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(medianX, top);
    ctx.lineTo(medianX, bottom);
    ctx.stroke();

    // The ruler: the Layer's index and the piece it placed.
    ctx.fillStyle = BAND_PALETTE.muted;
    ctx.font = "10px ui-monospace, monospace";
    ctx.textAlign = "right";
    ctx.textBaseline = "middle";
    if (geometry.rowHeight >= 9) {
      ctx.fillText(`${row.layer} ${row.piece}`, GUTTER_LEFT - 4, top + geometry.cellHeight / 2);
    }
    // The kept count, at the right edge — the engine's number, in words.
    ctx.textAlign = "left";
    ctx.fillStyle = BAND_PALETTE.muted;
    if (geometry.rowHeight >= 9) {
      ctx.fillText(`${row.count} kept`, GUTTER_LEFT + geometry.plotWidth + 6, top + geometry.cellHeight / 2);
    }
  }

  private drawLineage(
    model: BandModel,
    geometry: Geometry,
    xOf: (u: number) => number,
    originY: number,
  ): void {
    if (model.lineage.length === 0) return;
    const ctx = this.ctx;
    const centerY = (point: LineagePoint): number =>
      originY + point.layer * geometry.rowHeight + geometry.rowHeight / 2;

    ctx.save();
    ctx.lineWidth = 1.4;
    ctx.strokeStyle = BAND_PALETTE.lineage;
    for (let index = 1; index < model.lineage.length; index += 1) {
      const from = model.lineage[index - 1];
      const to = model.lineage[index];
      // A clipped point has no parent in the trace: a dashed segment says the
      // lineage has a hole, a solid one would claim it does not.
      ctx.setLineDash(to.clipped ? [3, 3] : []);
      ctx.beginPath();
      ctx.moveTo(xOf(from.u), centerY(from));
      ctx.lineTo(xOf(to.u), centerY(to));
      ctx.stroke();
    }
    ctx.setLineDash([]);
    for (const point of model.lineage) {
      const x = xOf(point.u);
      const y = centerY(point);
      ctx.beginPath();
      ctx.arc(x, y, 2.6, 0, Math.PI * 2);
      ctx.fillStyle = BAND_PALETTE.lineage;
      // A dark rim keeps a dot visible on top of a dense mark.
      ctx.strokeStyle = BAND_PALETTE.lineageEdge;
      ctx.lineWidth = 1;
      ctx.fill();
      ctx.stroke();
    }
    ctx.restore();
  }
}
