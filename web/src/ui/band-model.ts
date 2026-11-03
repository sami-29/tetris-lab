// The band as a pure model: geometry and marks, no DOM, no canvas, no wasm.
//
// §2.3's picture is "one row per Layer, states placed by score, with the winning
// lineage drawn through it". This module is that sentence as a data structure, so
// `web/test/trace-fixture.test.mjs` can assert it with no solver and no browser
// (`web/src/ui/band-canvas.ts` is the half that draws it, and draws nothing this
// module did not compute).
//
// Four rules, each of them a claim the ticket makes:
//
// 1. **One row per Layer**, top = layer 0 (the start position), bottom = the last
//    recorded Layer.
// 2. **Placed by score, never by parent and never by rank.** `u` is a function of
//    the score alone, on one axis shared by every row, so a row's place is
//    comparable with any other row's. Ordering by parent reads as arbitrary seams
//    (§2.3) and ordering by rank would draw a flat comb, because consecutive
//    ranks differ by ~0.1 score points.
// 3. **The lineage is drawn from its own records, one point per row**, even where
//    its rank is nowhere near the front. "Follow the leader" is wrong here — the
//    path leaves the front and comes back — and a greedy descent dead-ends, which
//    `greedySpine` returns so the page can say so.
// 4. **Every number is the engine's.** Counts are per-Layer *kept states* from the
//    trace, never a JavaScript recount of candidates or dedup survivors: §4.4's
//    five words carry no such thing, and inventing them would print a figure no
//    engine produced.

import { PARENT_NONE, PARENT_UNRECORDED, layerSlice, recordAt, type Trace, type TraceRecord } from "./trace.ts";
import { decodeTrace, layerSlice as layerSliceOf } from "./trace.ts";

export interface LineagePoint {
  layer: number;
  /** The record's rank in its layer — where it sits, not where it "should" be. */
  rank: number;
  score: number;
  lines: number;
  /** The record's own parent rank (`-1` at layer 0, `-2` when the band cut it). */
  parentRank: number;
  /** `true` when the parent's record is not in the trace: the lineage breaks here. */
  clipped: boolean;
  /** `true` when `rank` is at or beyond the drawing bound (the band by default). */
  outsideBound: boolean;
  /** Normalised place on the shared score axis; `0` is the best score. Set by `buildBand`. */
  u: number;
}

export interface BandMark {
  score: number;
  /** How many states of this row share the score — one mark per distinct score. */
  count: number;
  /** Normalised place on the shared axis; `0` is the best score. */
  u: number;
}

export interface BandRow {
  layer: number;
  /** The piece this Layer placed (`sequence[layer - 1]`), or `root` at layer 0. */
  piece: string;
  /** Records this Layer kept — the engine's count, `perLayerCounts[layer]`. */
  count: number;
  best: number;
  worst: number;
  median: number;
  marks: BandMark[];
  /** This row's lineage rank, or `null` when the lineage does not reach the row. */
  lineageRank: number | null;
}

export interface BandAxis {
  /** Worst score in the trace: the axis' left end is the best, so this is `u = 1`. */
  min: number;
  max: number;
  /** Round-ish score values to label, best to worst. */
  ticks: number[];
}

export interface BandModel {
  band: number;
  layerCount: number;
  /** Records the trace holds, i.e. §4.4's word 0. */
  records: number;
  counts: number[];
  axis: BandAxis;
  rows: BandRow[];
  lineage: LineagePoint[];
  /** Lineage points at or beyond the drawing bound. Zero when `bound === band`. */
  outsideBound: number;
  /** Lineage points whose parent the band did not keep. Zero when `band >= beam`. */
  clipped: number;
  /** One line for the inspector: what the picture is, in words, with its numbers. */
  label: string;
  caption: string;
}

export interface BandOptions {
  /**
   * The bound the lineage is drawn *through*, in records per row. Defaults to the
   * trace's own band. A drill-down (ticket 12's top-24) passes its own, and the
   * lineage must still be drawn outside it — at its own score, not clamped.
   */
  bound?: number;
  /** The Sequence, so a row can be labelled with the piece it placed. */
  sequence?: string;
  /** For the honesty line: the beam the build shipped, to compare the band with. */
  shippedBeam?: number;
}

/** The axis is `[worst, best]` with the best score on the **left**. */
function positionOf(score: number, min: number, max: number): number {
  if (max === min) return 0;
  return (max - score) / (max - min);
}

/** A median of a row's scores; the lower middle for an even count, never interpolated. */
function medianScore(scores: number[]): number {
  if (scores.length === 0) return 0;
  // `scores` arrives in rank order, i.e. non-increasing, so it is already sorted.
  return scores[Math.floor((scores.length - 1) / 2)];
}

/** Round-ish labelled ticks across the axis, best (left) to worst (right). */
function ticksFor(min: number, max: number, count = 5): number[] {
  if (max === min) return [min];
  const span = max - min;
  const raw = span / (count - 1);
  const magnitude = Math.pow(10, Math.floor(Math.log10(raw)));
  const step = Math.max(1, [1, 2, 5, 10].map((factor) => factor * magnitude).find((size) => size >= raw) ?? magnitude * 10);
  const ticks: number[] = [];
  for (let value = Math.ceil(min / step) * step; value <= max; value += step) ticks.push(value);
  // Best-first, because the axis reads left to right from best to worst.
  ticks.reverse();
  if (ticks.length === 0) ticks.push(max, min);
  return ticks;
}

/**
 * The lineage that ends as the best final state, walked back to the root.
 *
 * The trace carries no field marking the played chain, so this is the only
 * definition the data supports (§4.4 has no board and no move): start at rank 0 of
 * the last recorded Layer — its best score — and follow `parent_rank`. The result
 * is one point per Layer, whatever ranks the path visits.
 */
export function winningLineage(trace: Trace): LineagePoint[] {
  if (trace.layerCount === 0) return [];
  const points: LineagePoint[] = [];
  let layer = trace.layerCount - 1;
  let record: TraceRecord | undefined = recordAt(trace, layer, 0);
  while (record !== undefined) {
    points.push({
      layer,
      rank: record.rank,
      score: record.score,
      lines: record.lines,
      parentRank: record.parentRank,
      clipped: record.parentRank === PARENT_UNRECORDED || (layer > 0 && record.parentRank < 0),
      outsideBound: false,
      u: 0,
    });
    if (layer === 0 || record.parentRank < 0) break;
    layer -= 1;
    record = recordAt(trace, layer, record.parentRank);
  }
  return points.reverse();
}

/**
 * The greedy descent: always the best child, from the root. It is *not* the
 * lineage, and on a real trace it dead-ends — which is why the picture must not
 * offer "follow the leader" (§2.3, story 31).
 */
export function greedySpine(trace: Trace): Array<{ layer: number; rank: number }> {
  const spine: Array<{ layer: number; rank: number }> = [];
  if (trace.layerCount === 0) return spine;
  let layer = 0;
  let rank = 0;
  const kids = kidsIndex(trace);
  for (;;) {
    spine.push({ layer, rank });
    const next = kids[layer]?.get(rank);
    if (next === undefined || layer + 1 >= trace.layerCount) break;
    layer += 1;
    rank = next; // the smallest rank among the children: the best-scoring child
  }
  return spine;
}

/** For each layer, `rank -> the smallest rank pointing at it` in the next layer. */
function kidsIndex(trace: Trace): Array<Map<number, number>> {
  const index: Array<Map<number, number>> = [];
  for (let layer = 0; layer + 1 < trace.layerCount; layer += 1) {
    const best = new Map<number, number>();
    for (const record of layerSlice(trace, layer + 1)) {
      const parent = record.parentRank;
      if (parent < 0) continue;
      const seen = best.get(parent);
      if (seen === undefined || record.rank < seen) best.set(parent, record.rank);
    }
    index[layer] = best;
  }
  return index;
}

/** Build the whole picture from a decoded trace. Pure: the trace is not mutated. */
export function buildBand(trace: Trace, options: BandOptions = {}): BandModel {
  const bound = options.bound ?? trace.band;
  const scores = trace.records.map((record) => record.score);
  const min = scores.length === 0 ? 0 : Math.min(...scores);
  const max = scores.length === 0 ? 0 : Math.max(...scores);
  const axis: BandAxis = { min, max, ticks: ticksFor(min, max) };

  const rows: BandRow[] = [];
  for (let layer = 0; layer < trace.layerCount; layer += 1) {
    const records = layerSlice(trace, layer);
    const marks: BandMark[] = [];
    for (const record of records) {
      const last = marks[marks.length - 1];
      if (last !== undefined && last.score === record.score) {
        last.count += 1;
      } else {
        marks.push({ score: record.score, count: 1, u: positionOf(record.score, min, max) });
      }
    }
    rows.push({
      layer,
      piece: layer === 0 ? "root" : pieceAt(options.sequence, layer),
      count: trace.perLayerCounts[layer],
      best: records.length > 0 ? records[0].score : 0,
      worst: records.length > 0 ? records[records.length - 1].score : 0,
      median: medianScore(records.map((record) => record.score)),
      marks,
      lineageRank: null,
    });
  }

  const lineage = winningLineage(trace).map((point) => ({
    ...point,
    u: positionOf(point.score, min, max),
    outsideBound: point.rank >= bound,
  }));
  let outsideBound = 0;
  let clipped = 0;
  for (const point of lineage) {
    if (rows[point.layer] !== undefined) rows[point.layer].lineageRank = point.rank;
    if (point.outsideBound) outsideBound += 1;
    if (point.clipped) clipped += 1;
  }

  const label =
    options.shippedBeam === undefined
      ? `traced at band ${trace.band}`
      : trace.band === options.shippedBeam
        ? `traced at band ${trace.band} = the shipped beam`
        : `traced at band ${trace.band} · the shipped beam is ${options.shippedBeam}`;

  return {
    band: trace.band,
    layerCount: trace.layerCount,
    records: trace.records.length,
    counts: [...trace.perLayerCounts],
    axis,
    rows,
    lineage,
    outsideBound,
    clipped,
    label,
    caption: `band ${trace.band} = the beam · ${trace.layerCount} layers · ${trace.records.length} records · one row per Layer, states placed by score`,
  };
}

/** The piece a layer placed, from the Sequence the engine generated. */
function pieceAt(sequence: string | undefined, layer: number): string {
  if (sequence === undefined || layer - 1 >= sequence.length || layer < 1) return "?";
  return sequence[layer - 1];
}

/** What a mark's count means, in words, for a tooltip. */
export function describeMark(row: BandRow, mark: BandMark, band: number): string {
  const rank = row.marks.indexOf(mark);
  return `layer ${row.layer} (${row.piece}) · score ${mark.score} · rank ${rank} of ${row.count} · ${mark.count} state${mark.count === 1 ? "" : "s"} at this score · band ${band}`;
}

/** The per-layer readout beside the band: kept states, and where the lineage sits. */
export function countLines(model: BandModel): string[] {
  return model.rows.map((row) => {
    const lineage = row.lineageRank === null ? "no lineage" : `lineage rank ${row.lineageRank}`;
    return `L${row.layer} ${row.piece} · ${row.count} kept · ${lineage}`;
  });
}

// One import for a consumer of the band: the decoder and the sentinels travel with
// the model, so the page and ticket 12's tree do not have to know the file split.
export { PARENT_NONE, PARENT_UNRECORDED, decodeTrace, layerSliceOf as layerSlice };
