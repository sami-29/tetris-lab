// The inspector's table, its sparkline and its log stamps: pure, so they are testable
// without a browser.
//
// Three figures, three rules:
//
//   * **the per-layer table** — one row per Layer, and every column is the trace's own
//     record (`layer`, the Sequence's piece, rank-0 `score`, `lines`, `perLayerCounts`).
//     No column is a recount: §4.4 has no "expanded", no "dedup survivors", so the table
//     cannot show them (notes/14 §2.7).
//   * **the sparkline** — the best score kept at each Layer, left to right, against the
//     scores' own range so the top of the box is the best Layer's score.
//   * **the log stamp** — one shape, `+16.6 ms`, because a log where some lines say
//     "0.4s" and others say "400ms" is a log nobody can read down.

import type { Trace } from "./trace.ts";

export interface LayerRow {
  readonly layer: number;
  /** The piece the Layer placed, or `root` for the position the game was given. */
  readonly piece: string;
  /** The best score kept in the Layer: the score of its rank-0 record. */
  readonly best: number;
  /** Cumulative Lines cleared to reach that state. */
  readonly lines: number;
  /** States kept in the Layer — the trace's own count. */
  readonly kept: number;
}

/**
 * Every Layer as one row. `sequence` is the engine's Sequence string
 * (`GameMessage.sequence`), so the piece column is the same letters the glyphs draw.
 */
export function layerRows(trace: Trace, sequence: string): LayerRow[] {
  const rows: LayerRow[] = [];
  for (let layer = 0; layer < trace.layerCount; layer += 1) {
    const start = trace.layerStart[layer];
    const best = trace.records[start];
    rows.push({
      layer,
      piece: layer === 0 ? "root" : (sequence[layer - 1] ?? "?"),
      best: best?.score ?? 0,
      lines: best?.lines ?? 0,
      kept: trace.perLayerCounts[layer] ?? 0,
    });
  }
  return rows;
}

export interface Sparkline {
  /** An SVG path for one polyline; empty when there is nothing to draw. */
  readonly path: string;
  readonly points: ReadonlyArray<{ x: number; y: number }>;
}

/**
 * `scores` as one polyline inside a `width` × `height` box.
 *
 * A flat series (one score, or every Layer equal) is drawn on the middle line rather
 * than at `NaN` — the same defensiveness the width slider's tick needed for its
 * degenerate case.
 */
export function sparkline(scores: readonly number[], width: number, height: number): Sparkline {
  if (scores.length === 0) return { path: "", points: [] };
  const best = Math.max(...scores);
  const worst = Math.min(...scores);
  const span = best - worst;
  const step = scores.length === 1 ? 0 : width / (scores.length - 1);
  const points = scores.map((score, index) => ({
    x: round(index * step),
    y: round(span === 0 ? height / 2 : ((best - score) / span) * height),
  }));
  const path = points.map((point, index) => `${index === 0 ? "M" : "L"}${point.x},${point.y}`).join(" ");
  return { path, points };
}

/** Two decimals is enough for a 54 px plot and keeps the attribute short. */
function round(value: number): number {
  return Math.round(value * 100) / 100;
}

/**
 * A log line's stamp: milliseconds since the page loaded, to a tenth, always signed
 * with the same `+` so the column reads as a column.
 */
export function logStamp(msSinceLoad: number): string {
  const safe = Number.isFinite(msSinceLoad) && msSinceLoad > 0 ? msSinceLoad : 0;
  return `+${safe.toFixed(1)} ms`;
}
