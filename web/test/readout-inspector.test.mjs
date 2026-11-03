// The centre's readout and the inspector's table, as pure models.
//
// The pictures are drawn by the DOM (`main.ts`), but three of the things on screen
// are arithmetic over the engine's own numbers, and arithmetic is testable without a
// browser:
//
//   * the **column profile** — ten bars, heights in rows, from the replayed Board;
//   * the **per-layer table** — one row per Layer, whose five columns are the
//     trace's records and the Sequence, never a recount;
//   * the **sparkline** — the best score kept at each Layer, left to right.
//
// The committed trace fixture is the input, so the expectations are the engine's
// recording and not a hand-written copy of it (`web/test/fixtures/
// trace-seed0-goal8-p40-cap7-band25.json`, 444 records over 20 Layers).

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";

import { columnProfile, profileTip } from "../src/ui/readout.ts";
import { layerRows, logStamp, sparkline } from "../src/ui/inspector.ts";
import { decodeTrace } from "../src/ui/trace.ts";

const WEB = resolve(fileURLToPath(new URL(".", import.meta.url)), "..");
const FIXTURE = JSON.parse(readFileSync(resolve(WEB, "test", "fixtures", "trace-seed0-goal8-p40-cap7-band25.json"), "utf8"));

const field = Uint32Array.from(FIXTURE.game.board.map((word) => Number(word)));
const trace = decodeTrace(FIXTURE.traceWords, FIXTURE.band);

test("the column profile is ten columns of rows, with the holes the stack hides", () => {
  const profile = columnProfile(field);
  assert.equal(profile.length, 10, "ten columns, always");
  assert.deepEqual(profile.map((column) => column.height), [0, 0, 0, 0, 4, 0, 2, 7, 7, 3],
    "height in rows: the top of the stack, floor-anchored");
  assert.deepEqual(profile.map((column) => column.holes), [0, 0, 0, 0, 0, 0, 1, 4, 0, 2],
    "holes are empty cells under a filled one, counted per column");
  // A bar is drawn against the tallest column on screen, so the picture has a scale
  // that the numbers also state.
  assert.equal(Math.max(...profile.map((column) => column.height)), 7, "the tallest column is the bar scale");
  assert.match(profileTip(7, profile[7]), /column 7 · height 7 rows · 4 holes/, "each column is tipped in words");
});

test("the per-layer table's five columns are the trace's, not a recount", () => {
  const rows = layerRows(trace, FIXTURE.game.sequence);
  assert.equal(rows.length, trace.layerCount, "one row per Layer");
  assert.deepEqual(rows[0], { layer: 0, piece: "root", best: trace.records[0].score, lines: trace.records[0].lines, kept: trace.perLayerCounts[0] },
    "layer 0 is the position the game was given");
  assert.equal(rows[7].piece, FIXTURE.game.sequence[6], "Layer 7 placed the Sequence's 7th piece");
  const layer7 = trace.records[trace.layerStart[7]];
  assert.equal(rows[7].best, layer7.score, "the best score is the layer's rank-0 record");
  assert.equal(rows[7].lines, layer7.lines, "and its Lines cleared are the record's");
  assert.equal(rows[7].kept, trace.perLayerCounts[7], "and the states kept are the trace's own count");
  assert.equal(rows.reduce((sum, row) => sum + row.kept, 0), trace.records.length, "the kept column sums to the record count");
});

test("the sparkline is one point per Layer, left to right, inside its box", () => {
  const scores = trace.perLayerCounts.map((_, layer) => trace.records[trace.layerStart[layer]].score);
  const line = sparkline(scores, 200, 54);
  assert.equal(line.points.length, scores.length, "one point per Layer");
  assert.equal(line.points[0].x, 0, "the first Layer is the left edge");
  assert.equal(line.points[line.points.length - 1].x, 200, "the last is the right edge");
  for (const point of line.points) {
    assert.ok(point.y >= 0 && point.y <= 54, "every point is inside the plot");
  }
  // The best score is the top of the box, the worst the bottom: the axis is the
  // scores themselves, not a padded range.
  const best = Math.max(...scores);
  const worst = Math.min(...scores);
  assert.equal(line.points[scores.indexOf(best)].y, 0, "the best score is drawn at the top");
  assert.equal(line.points[scores.indexOf(worst)].y, 54, "the worst at the bottom");
  assert.match(line.path, /^M/, "a path, so the SVG draws one polyline");
  assert.deepEqual(sparkline([], 200, 54), { path: "", points: [] }, "no trace, no line — and no NaN");
  assert.equal(sparkline([5, 5], 200, 54).points[0].y, 27, "one score is drawn in the middle, not at y=NaN");
});

test("the log stamps milliseconds since the page loaded, in one shape", () => {
  assert.equal(logStamp(0), "+0.0 ms");
  assert.equal(logStamp(16.64), "+16.6 ms");
  assert.equal(logStamp(1234.5), "+1234.5 ms");
  assert.equal(logStamp(-1), "+0.0 ms", "a negative clock is a boot artefact, not a figure");
});
