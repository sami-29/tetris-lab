// The trace decoder: §4.4's words in, a `Trace` out.
//
// This is the one place the band, the per-layer counts and (ticket 12) the bounded
// tree get their data. It is deliberately a pure function of an `Int32Array`:
//
//   * the live page hands it the words the worker copied out of the wasm region;
//   * `web/test/trace-fixture.test.mjs` hands it the committed fixture's words —
//     **the same decoder**, so a fixture test exercises production code and not a
//     prettified copy of the data;
//   * nothing here re-derives a number the engine already said. There is no
//     scoring, no Board, no expansion: five words per record, in the order §4.4
//     fixed, and per-layer counts read off the layer field.
//
// §4.4's layout, verbatim:
//
//   word 0            record count n
//   words 1..1+5n     records: [layer, rank, score, lines, parent_rank] as i32
//
// `rank` is 0-based within its layer (0 = the best score), the records are in layer
// order then rank order, and `parent_rank` is the parent's rank in the previous
// layer — `-1` at layer 0 (§4.4), `-2` when a band narrower than the beam did not
// keep the parent (ticket 11's second sentinel: a *named* missing edge, never a
// dangling pointer).

/** §4.4: `parent_rank` at layer 0, where nothing placed the start position. */
export const PARENT_NONE = -1;

/** The parent exists in the previous layer but the band cut its record. */
export const PARENT_UNRECORDED = -2;

export interface TraceRecord {
  layer: number;
  /** 0-based within its layer; 0 is the best score. */
  rank: number;
  /** The Evaluator's score for the state, `i64 >> 16` as §4.4's `i32`. */
  score: number;
  /** Cumulative Lines cleared to reach the state. */
  lines: number;
  /** The parent's rank in the previous layer; `-1` / `-2` are sentinels. */
  parentRank: number;
}

export interface Trace {
  /** Layer order, then rank order (§4.4). */
  records: TraceRecord[];
  /** Records per layer, in layer order. */
  perLayerCounts: number[];
  /** Index of each layer's first record, so a row is a slice and never a scan. */
  layerStart: number[];
  layerCount: number;
  /** The band the trace was recorded at: §4.4's record cap per layer. */
  band: number;
}

/**
 * Decode §4.4's words. Total: a short, ragged or empty word stream yields a
 * `Trace` rather than throwing, because a trace is a recording and a recording can
 * be truncated (`n` is the only terminator, and word 0 is written last).
 */
export function decodeTrace(words: ArrayLike<number>, band: number = Number.POSITIVE_INFINITY): Trace {
  const count = words.length > 0 ? Math.max(0, Math.min(words[0] | 0, Math.floor((words.length - 1) / 5))) : 0;
  const records: TraceRecord[] = new Array(count);
  const perLayerCounts: number[] = [];
  const layerStart: number[] = [];
  let layer = -1;
  for (let index = 0; index < count; index += 1) {
    const at = 1 + index * 5;
    const record: TraceRecord = {
      layer: words[at],
      rank: words[at + 1],
      score: words[at + 2],
      lines: words[at + 3],
      parentRank: words[at + 4],
    };
    if (record.layer !== layer || index === 0) {
      // §4.4 orders the records by layer, so a new layer is a new run. A record
      // that *skips* a layer would leave a hole; close it with empty layers so
      // `layerCount` never lies about the geometry.
      for (let missing = layer + 1; missing < record.layer; missing += 1) {
        layerStart[missing] = index;
        perLayerCounts[missing] = 0;
      }
      layer = record.layer;
      layerStart[layer] = index;
      perLayerCounts[layer] = 0;
    }
    perLayerCounts[layer] += 1;
    records[index] = record;
  }
  return {
    records,
    perLayerCounts,
    layerStart,
    layerCount: perLayerCounts.length,
    band,
  };
}

/** One layer's records, in rank order. A slice: never a sort, never a filter. */
export function layerSlice(trace: Trace, layer: number): TraceRecord[] {
  if (layer < 0 || layer >= trace.layerCount) return [];
  const start = trace.layerStart[layer];
  return trace.records.slice(start, start + trace.perLayerCounts[layer]);
}

/** The record at `(layer, rank)`, or `undefined` when it is not in the trace. */
export function recordAt(trace: Trace, layer: number, rank: number): TraceRecord | undefined {
  if (layer < 0 || layer >= trace.layerCount) return undefined;
  if (rank < 0 || rank >= trace.perLayerCounts[layer]) return undefined;
  return trace.records[trace.layerStart[layer] + rank];
}
