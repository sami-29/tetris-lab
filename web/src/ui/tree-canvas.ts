// The bounded tree, drawn. One `canvas`, one render function, no state of its own.
//
// This file is deliberately thin: every position, every opacity and every mark it
// draws comes from `tree-model.ts` — `treeDisplayList` is the whole input — so the
// picture cannot disagree with the numbers the inspector prints, and the whole
// visual decision lives in `TREE_PALETTE` below. Ticket 14 owns the workbench's
// visual system and replaces that block; nothing else here needs to move.
//
// What it draws, bottom of the stack to the top:
//
//   * the ruler: `L4 T` every few columns, so labels never collide, and the kept
//     count on the selected column only ("24 of 25 kept" — the engine's number);
//   * the edges, dimmed to 0.05 when they are off the selected path (the prototype
//     got this wrong: its dim left every edge at full strength);
//   * one primary mark per state — an 8-px dot, or a **tick** when the search
//     dropped it — plus its **stem** (its parent is off-view, and which kind of
//     off-view is in the tooltip) and its **chevron** (its children are below the
//     bound);
//   * the selected state's white ring, and the amber of the winning lineage.
//
// Sizes come from the model's layout, so the legibility arithmetic (24 rows at a
// 31-px pitch, an 8-px dot) is one place and is asserted by the test, not read off
// the pixels afterwards.

import { TREE_COPY, treeDisplayList, type TreeMark, type TreeModel } from "./tree-model.ts";

/**
 * The tree's whole colour system, in one place.
 *
 * Ticket 14 replaces this object with the workbench's tokens; the drawing code
 * reads nothing else. The teals are the band's (the two views are one picture at
 * two magnifications) and amber means the winning lineage and nothing else.
 */
export const TREE_PALETTE = {
  background: "#0b0f14",
  panel: "#121a23",
  grid: "rgba(44, 50, 56, 0.8)",
  ink: "#dbe6f2",
  muted: "#8ea3c0",
  /** A surviving state, by score tier (the band's teals). */
  state: ["rgba(61, 143, 138, 0.72)", "rgba(95, 192, 184, 0.92)", "rgba(140, 224, 214, 0.95)"] as const,
  /** A dark rim keeps a dot legible on top of a dense column. */
  stateEdge: "#0b0f14",
  /** Edges. The model supplies the opacity, so the dim reaches them. */
  edge: "rgba(93, 143, 139, 1)",
  /** The three "nothing leaves / nothing enters / below the bound" marks. */
  tick: "rgba(120, 132, 143, 1)",
  chevron: "rgba(120, 132, 143, 1)",
  stem: "rgba(120, 132, 143, 1)",
  /** The winning lineage: a colour nothing else in the tree uses. */
  lineage: "rgba(224, 163, 62, 0.95)",
  /** The selected state's ring. */
  selected: "#ffffff",
} as const;

/** How far right of a state its chevron sits, in CSS pixels. */
const CHEVRON_GAP = 4;
/** The stem's length, entering from the column's left gutter. */
const STEM_LENGTH = 3;
/** The tick's bar, in CSS pixels. */
const TICK_WIDTH = 2;
const TICK_HEIGHT = 7;

export class TreeView {
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
  draw(model: TreeModel | null): void {
    const rect = this.canvas.getBoundingClientRect();
    const width = Math.max(1, Math.floor(rect.width));
    const height = Math.max(1, Math.floor(rect.height));
    const ctx = this.ctx;
    ctx.globalAlpha = 1;
    ctx.fillStyle = TREE_PALETTE.background;
    ctx.fillRect(0, 0, width, height);
    if (model === null || model.nodes.length === 0) {
      this.canvas.dataset.nodes = "0";
      ctx.fillStyle = TREE_PALETTE.muted;
      ctx.font = "11px ui-monospace, monospace";
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(TREE_COPY.emptyTrace, width / 2, height / 2);
      return;
    }

    const { layout } = model;
    this.drawRuler(model, width);

    const marks = treeDisplayList(model);
    const span = scoreSpan(model);
    ctx.save();
    ctx.beginPath();
    ctx.rect(layout.originX - 6, layout.originY - 2, layout.plotWidth + 12, layout.plotHeight + 4);
    ctx.clip();
    for (const mark of marks) if (mark.kind === "edge") this.drawEdge(model, mark);
    for (const mark of marks) if (mark.kind !== "edge") this.drawMark(model, mark, span);
    ctx.restore();

    this.drawAxis(model, width, height);

    // Geometry a headless check can read back out of the attributes.
    this.canvas.dataset.nodes = String(model.nodes.length);
    this.canvas.dataset.bound = String(model.bound);
    this.canvas.dataset.band = String(model.band);
    this.canvas.dataset.ticks = String(model.counts.perLayerTicks.reduce((sum, value) => sum + value, 0));
    this.canvas.dataset.stems = String(model.counts.perLayerStems.reduce((sum, value) => sum + value, 0));
    this.canvas.dataset.chevrons = String(model.counts.perLayerChevrons.reduce((sum, value) => sum + value, 0));
    this.canvas.dataset.rowPitch = String(layout.rowPitch);
    this.canvas.dataset.selection =
      model.selection === null ? "" : `${model.selection.layer}:${model.selection.rank}`;
  }

  /** `L4 T` on every column that fits, and the kept count on the selected one. */
  private drawRuler(model: TreeModel, width: number): void {
    const ctx = this.ctx;
    const { layout } = model;
    const step = Math.max(1, Math.ceil(46 / Math.max(1, layout.colPitch)));
    ctx.save();
    ctx.font = "10px ui-monospace, monospace";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    for (const column of model.columns) {
      const index = column.layer - model.window.from;
      if (index % step !== 0) continue;
      const selected = model.selection !== null && model.selection.layer === column.layer;
      ctx.globalAlpha = selected ? 1 : 0.85;
      ctx.fillStyle = selected ? TREE_PALETTE.ink : TREE_PALETTE.muted;
      ctx.fillText(column.label, column.x, layout.originY - 8);
      if (selected) {
        ctx.globalAlpha = 0.85;
        ctx.fillStyle = TREE_PALETTE.muted;
        ctx.fillText(`${column.drawn} of ${column.count} kept`, column.x, layout.originY - 20);
      }
    }
    ctx.globalAlpha = 1;
    // The window, in words, when the trace holds more Layers than fit.
    if (model.window.count < model.window.total) {
      ctx.fillStyle = TREE_PALETTE.muted;
      ctx.textAlign = "left";
      ctx.fillText(
        `← ${model.window.count} of ${model.window.total} Layers →`,
        layout.originX,
        layout.originY - 8,
      );
    }
    ctx.restore();
    this.canvas.dataset.rulerWidth = String(width);
  }

  private drawAxis(model: TreeModel, width: number, height: number): void {
    const ctx = this.ctx;
    ctx.save();
    ctx.strokeStyle = TREE_PALETTE.grid;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(model.layout.originX - 6, height - 18);
    ctx.lineTo(width - 8, height - 18);
    ctx.stroke();
    ctx.fillStyle = TREE_PALETTE.muted;
    ctx.font = "10px ui-monospace, monospace";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(TREE_COPY.axis, width / 2, height - 8);
    ctx.restore();
  }

  private drawEdge(model: TreeModel, mark: TreeMark): void {
    const ctx = this.ctx;
    const from = model.nodes.find((node) => node.layer === mark.from?.layer && node.rank === mark.from?.rank);
    const to = model.nodes.find((node) => node.layer === mark.to?.layer && node.rank === mark.to?.rank);
    if (from === undefined || to === undefined) return;
    ctx.save();
    ctx.globalAlpha = mark.alpha;
    ctx.strokeStyle = mark.lineage ? TREE_PALETTE.lineage : TREE_PALETTE.edge;
    ctx.lineWidth = mark.lineage ? 1.4 : 1;
    ctx.beginPath();
    // Start and end at the marks' edges, not their centres, so a dot is never
    // half-covered by its own edges.
    const radius = model.layout.nodeDiameter / 2;
    ctx.moveTo(from.x, from.y + (to.y > from.y ? radius : -radius));
    ctx.lineTo(to.x, to.y + (to.y > from.y ? -radius : radius));
    ctx.stroke();
    ctx.restore();
  }

  private drawMark(model: TreeModel, mark: TreeMark, span: ScoreSpan): void {
    const ctx = this.ctx;
    const radius = model.layout.nodeDiameter / 2;
    ctx.save();
    ctx.globalAlpha = mark.alpha;
    switch (mark.kind) {
      case "state": {
        const node = model.nodes.find((candidate) => candidate.id === mark.id);
        const tier = node === undefined ? 1 : scoreTier(span, node.score);
        ctx.fillStyle = mark.lineage ? TREE_PALETTE.lineage : TREE_PALETTE.state[tier];
        ctx.strokeStyle = TREE_PALETTE.stateEdge;
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.arc(mark.x, mark.y, radius, 0, Math.PI * 2);
        ctx.fill();
        ctx.stroke();
        break;
      }
      case "tick":
        // A bar, not a dot: "the search did not extend this board".
        ctx.fillStyle = TREE_PALETTE.tick;
        ctx.fillRect(mark.x - TICK_WIDTH / 2, mark.y - TICK_HEIGHT / 2, TICK_WIDTH, TICK_HEIGHT);
        break;
      case "stem": {
        // A stub entering from the column's left edge: something came from off-view.
        ctx.fillStyle = TREE_PALETTE.stem;
        ctx.fillRect(mark.x - radius - STEM_LENGTH - 2, mark.y - 1, STEM_LENGTH, 2);
        break;
      }
      case "chevron": {
        // A chevron 4 px right of the state: its children are below the bound.
        const x = mark.x + radius + CHEVRON_GAP;
        ctx.strokeStyle = TREE_PALETTE.chevron;
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.moveTo(x, mark.y - 3);
        ctx.lineTo(x + 3, mark.y);
        ctx.lineTo(x, mark.y + 3);
        ctx.stroke();
        break;
      }
      default:
        break;
    }
    if (mark.selected) {
      ctx.globalAlpha = 1;
      ctx.strokeStyle = TREE_PALETTE.selected;
      ctx.lineWidth = 1.4;
      ctx.beginPath();
      ctx.arc(mark.x, mark.y, radius + 3, 0, Math.PI * 2);
      ctx.stroke();
    }
    ctx.restore();
  }
}

interface ScoreSpan {
  best: number;
  worst: number;
}

/** The drawn states' own score span, so the three teals mean one thing per frame. */
function scoreSpan(model: TreeModel): ScoreSpan {
  let best = Number.NEGATIVE_INFINITY;
  let worst = Number.POSITIVE_INFINITY;
  for (const node of model.nodes) {
    if (node.score > best) best = node.score;
    if (node.score < worst) worst = node.score;
  }
  return { best, worst };
}

/** Which of the three teals a state's score earns, across that span. */
function scoreTier(span: ScoreSpan, score: number): 0 | 1 | 2 {
  if (!Number.isFinite(span.best) || span.best === span.worst) return 2;
  const third = (span.best - span.worst) / 3;
  if (score >= span.best - third) return 2;
  return score >= span.best - 2 * third ? 1 : 0;
}
