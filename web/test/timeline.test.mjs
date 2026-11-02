// The playback timeline: replaying the engine's Move list must reproduce the
// engine's boards, word for word, Move for Move.
//
// The page cannot ask wasm for "the board after move k" — §4 has no such export,
// and `solve` leaves the board buffer untouched — so `web/src/ui/timeline.ts`
// replays `(starting Board, Move list)` itself. That makes the page a second
// implementation of gravity, placement and clearing, and a second implementation
// drifts. The leash is `web/test/fixtures/replay-vectors.json`: written by
// `web/scripts/gen-replay-vectors`, which links **the engine crate itself** and
// records the ten board words after every Move. This test compares the JavaScript
// replay against those words one Move at a time, so "a seek reconstructs exactly
// the position the search had" is a checked claim and not a hope.
//
// The fixture is generated, never hand-edited: `cd web/scripts/gen-replay-vectors
// && cargo run --release`.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import {
  applyPlan,
  buildTimeline,
  collapsePlan,
  reconstruct,
  unpackMove,
} from "../src/ui/timeline.ts";
import { H, W, fieldFrom, fullRows, clearFullRows, place, restShift } from "../src/ui/field.ts";
import { PIECE_LETTERS, rotationOf } from "../src/ui/pieces.ts";

const FIXTURE = JSON.parse(
  readFileSync(fileURLToPath(new URL("./fixtures/replay-vectors.json", import.meta.url)), "utf8"),
);

/** §4.1.1: `(piece_index << 16) | (rotation << 8) | column`. */
const word = ([piece, rotation, column]) => ((piece << 16) | (rotation << 8) | column) >>> 0;
const words = (field) => [...field];
const cellCount = (field) => [...field].reduce((sum, column) => sum + popcount(column), 0);
const popcount = (value) => {
  let count = 0;
  for (let bits = value >>> 0; bits !== 0; bits &= bits - 1) count += 1;
  return count;
};
const rowsOf = (field) => {
  const rows = [];
  for (let col = 0; col < W; col += 1) {
    for (let bit = 0; bit < H; bit += 1) {
      if ((field[col] & (1 << bit)) !== 0) rows.push(H - 1 - bit);
    }
  }
  return rows;
};

test("the golden fixture is generated and holds the cases the replay must survive", () => {
  assert.ok(Array.isArray(FIXTURE.cases) && FIXTURE.cases.length >= 6, "six or more cases");
  const names = FIXTURE.cases.map((entry) => entry.name);
  for (const name of ["four-row", "one-row", "mid-stack", "non-contiguous", "empty-no-clear"]) {
    assert.ok(names.includes(name), `the fixture has the ${name} case`);
  }
  assert.ok(
    names.some((name) => name.startsWith("game-seed")),
    "and at least one real solved game",
  );
  for (const entry of FIXTURE.cases) {
    assert.equal(entry.frames.length, entry.moves.length, `${entry.name}: a board per Move`);
    assert.equal(entry.cleared.length, entry.moves.length, `${entry.name}: a clear list per Move`);
    assert.equal(entry.board0.length, W, `${entry.name}: ten column words`);
  }
});

test("the replay matches the engine word for word, Move for Move", () => {
  for (const entry of FIXTURE.cases) {
    const timeline = buildTimeline(entry.board0, entry.moves.map(word));
    assert.equal(timeline.steps.length, entry.moves.length, `${entry.name}: a step per Move`);

    for (const [index, step] of timeline.steps.entries()) {
      const label = `${entry.name} move ${index + 1}`;
      assert.deepEqual(words(reconstruct(timeline, index + 1)), entry.frames[index], `${label}: board`);
      assert.equal(step.k, index + 1, `${label}: k is 1-based`);
      assert.deepEqual(step.cleared, entry.cleared[index], `${label}: cleared rows`);
      assert.equal(step.restShift, entry.shifts[index], `${label}: the engine's resting shift`);
      assert.equal(step.dropRows, entry.drops[index], `${label}: rows fallen`);
      assert.equal(step.linesAfter, entry.lines[index], `${label}: cumulative Lines`);
      assert.equal(step.linesBefore, index === 0 ? 0 : entry.lines[index - 1], `${label}: Lines before`);
    }

    assert.equal(timeline.lines, entry.lines.at(-1) ?? 0, `${entry.name}: total Lines`);
    assert.deepEqual(
      words(reconstruct(timeline, 0)),
      entry.board0,
      `${entry.name}: move 0 is the starting Board`,
    );
  }
});

test("a Move's own boards explain it: preBoard, preClear and postClear are contiguous", () => {
  for (const entry of FIXTURE.cases) {
    const timeline = buildTimeline(entry.board0, entry.moves.map(word));
    for (const [index, step] of timeline.steps.entries()) {
      const label = `${entry.name} move ${index + 1}`;
      // preBoard is the board the Move is applied to, and nothing else.
      assert.deepEqual(words(step.preBoard), words(reconstruct(timeline, index)), `${label}: preBoard`);
      // preClear is preBoard plus the piece at rest: the placement, before clearing.
      const shape = rotationOf(step.piece, step.rotation);
      assert.ok(shape, `${label}: the glyph table has the rotation`);
      const stamped = fieldFrom(step.preBoard);
      place(stamped, shape, step.column, step.restShift);
      assert.deepEqual(words(step.preClear), words(stamped), `${label}: preClear is the placement`);
      // The rows it completed are exactly the full rows of preClear.
      const full = fullRows(step.preClear);
      const filled = [];
      for (let bit = H - 1; bit >= 0; bit -= 1) if ((full & (1 << bit)) !== 0) filled.push(H - 1 - bit);
      assert.deepEqual(filled, step.cleared, `${label}: cleared is every full row of preClear`);
      // And postClear is preClear with them removed.
      assert.deepEqual(words(step.postClear), words(reconstruct(timeline, index + 1)), `${label}: postClear`);
      assert.ok(step.cleared.length <= 4, `${label}: a placement clears at most four rows`);
    }
  }
});

test("a cleared row set is not necessarily contiguous: each surviving segment falls its own distance", () => {
  const entry = FIXTURE.cases.find((one) => one.name === "non-contiguous");
  assert.ok(entry, "the fixture carries the non-contiguous case");
  const timeline = buildTimeline(entry.board0, entry.moves.map(word));
  const step = timeline.steps[0];
  assert.deepEqual(step.cleared, [14, 17], "rows 14 and 17, with 15 and 16 left unfilled");

  // Three segments, three fall distances: rows 0..13 fall 2, rows 15..16 fall 1,
  // rows 18..19 do not move. A single falling band cannot draw this.
  assert.deepEqual(step.plan.segments, [
    { from: 0, to: 13, rows: 14, fall: 2 },
    { from: 15, to: 16, rows: 2, fall: 1 },
    { from: 18, to: 19, rows: 2, fall: 0 },
  ]);
  assert.deepEqual(step.plan.vanish, [14, 17]);
  assert.equal(step.plan.moves.length, H - step.cleared.length, "every surviving row has a move");

  // The plan, applied to preClear, is postClear: the renderer's segments are the
  // board's own cells, not a decorative approximation.
  assert.deepEqual(words(applyPlan(step.preClear, step.plan)), words(step.postClear));

  // A piece's cells are contiguous in its box but the rows it *completes* are not.
  const top = step.plan.segments[0];
  assert.equal(top.fall, 2, "the stack above both cleared rows falls two rows");
});

test("collapsePlan is clearFullRows, on arbitrary boards including non-contiguous full rows", () => {
  // A deterministic LCG so the boards are the same on every run.
  let state = 0x2545f491;
  const next = (bound) => {
    state = (Math.imul(state ^ (state >>> 15), 0x2c1b3c6d) ^ (state >>> 12)) >>> 0;
    return state % bound;
  };

  let sawNonContiguous = 0;
  let sawClearing = 0;
  for (let trial = 0; trial < 4000; trial += 1) {
    const base = fieldFrom([0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    // A random stack, then a few planted full rows (sometimes adjacent, sometimes not).
    for (let col = 0; col < W; col += 1) {
      const height = next(6);
      let column = 0;
      for (let bit = 0; bit < height; bit += 1) column |= 1 << bit;
      base[col] = column >>> 0;
    }
    const planted = [];
    for (let attempt = 0; attempt < 3; attempt += 1) {
      const row = next(H);
      if (planted.includes(row)) continue;
      planted.push(row);
      for (let col = 0; col < W; col += 1) base[col] = (base[col] | (1 << (H - 1 - row))) >>> 0;
    }
    planted.sort((a, b) => a - b);

    // The rows that are actually full, top first: the random stack can complete
    // rows of its own, and those are part of the case.
    const full = fullRows(base);
    const clearedRows = [];
    for (let bit = H - 1; bit >= 0; bit -= 1) if ((full & (1 << bit)) !== 0) clearedRows.push(H - 1 - bit);

    const expected = fieldFrom(base);
    const clearedCount = clearFullRows(expected);
    assert.equal(clearedCount, clearedRows.length, "the number cleared is the number that were full");

    const plan = collapsePlan(clearedRows);
    assert.deepEqual(words(applyPlan(base, plan)), words(expected), "plan == splice");
    assert.deepEqual(plan.vanish, clearedRows, "the plan vanishes exactly the cleared rows");
    assert.equal(plan.moves.length, H - clearedRows.length, "one move per surviving row");

    // Every surviving row appears once and its destination is inside the well. The
    // destinations are distinct: a clear funnels the survivors into the bottom
    // `H - cleared` rows, never into an occupied one.
    const sources = plan.moves.map((move) => move.from);
    const destinations = plan.moves.map((move) => move.to);
    const surviving = [];
    for (let row = 0; row < H; row += 1) if (!clearedRows.includes(row)) surviving.push(row);
    assert.deepEqual([...sources].sort((a, b) => a - b), surviving, "every surviving row moves");
    const bottom = [];
    for (let row = clearedRows.length; row < H; row += 1) bottom.push(row);
    assert.deepEqual(
      [...destinations].sort((a, b) => a - b),
      bottom,
      "the survivors settle into the bottom rows, one each",
    );
    // A row's fall is the number of cleared rows below it: rows never rise.
    for (const move of plan.moves) {
      assert.equal(
        move.to - move.from,
        clearedRows.filter((row) => row > move.from).length,
        `row ${move.from} falls past the cleared rows under it`,
      );
      assert.ok(move.to >= move.from, "gravity only pulls down");
    }
    // The segments partition the surviving rows and each carries one fall distance.
    let covered = 0;
    for (const segment of plan.segments) {
      assert.ok(segment.to >= segment.from, "a segment is a run of rows");
      assert.equal(segment.rows, segment.to - segment.from + 1, "a segment's row count is its span");
      for (let row = segment.from; row <= segment.to; row += 1) {
        assert.ok(!clearedRows.includes(row), "a segment never carries a vanished row");
        assert.equal(
          clearedRows.filter((other) => other > row).length,
          segment.fall,
          "one fall distance per segment",
        );
      }
      covered += segment.rows;
    }
    assert.equal(covered, H - clearedRows.length, "the segments partition the surviving rows");

    // Segments are ordered top-down, so the renderer draws in screen order.
    for (let index = 1; index < plan.segments.length; index += 1) {
      assert.ok(plan.segments[index - 1].to < plan.segments[index].from, "segments are ordered");
    }

    if (clearedRows.length > 0) sawClearing += 1;
    // One segment per run of surviving rows: a cleared row splits the stack.
    let runs = 0;
    for (let row = 0; row < H; row += 1) {
      if (clearedRows.includes(row)) continue;
      if (row === 0 || clearedRows.includes(row - 1)) runs += 1;
    }
    assert.equal(plan.segments.length, runs, "a cleared row splits the stack into segments");
    if (clearedRows.some((row, index) => index > 0 && clearedRows[index - 1] !== row - 1)) {
      sawNonContiguous += 1;
    }
  }
  assert.ok(sawClearing > 1000, "the sweep actually cleared rows");
  assert.ok(sawNonContiguous > 10, "and it met plenty of non-contiguous clears");
});

test("a mid-stack clear leaves the rows below it alone", () => {
  const entry = FIXTURE.cases.find((one) => one.name === "mid-stack");
  assert.ok(entry);
  const timeline = buildTimeline(entry.board0, entry.moves.map(word));
  const step = timeline.steps[0];
  const lowest = Math.max(...step.cleared);
  assert.ok(lowest < H - 1, "the clear is above the floor");

  for (const move of step.plan.moves) {
    if (move.from > lowest) {
      assert.equal(move.to, move.from, `row ${move.from} is below the clear and does not move`);
      // Its cells are untouched: the same row, same columns, before and after.
      for (let col = 0; col < W; col += 1) {
        const bit = H - 1 - move.from;
        assert.equal(
          (step.preClear[col] | 0) & (1 << bit) ? 1 : 0,
          (step.postClear[col] | 0) & (1 << bit) ? 1 : 0,
          `row ${move.from} column ${col} keeps its cell`,
        );
      }
    }
  }
});

test("no-clear steps are the identity: nothing vanishes and nothing falls", () => {
  const entry = FIXTURE.cases.find((one) => one.name === "empty-no-clear");
  assert.ok(entry);
  const timeline = buildTimeline(entry.board0, entry.moves.map(word));
  for (const step of timeline.steps) {
    assert.deepEqual(step.cleared, [], "nothing was cleared");
    assert.deepEqual(step.plan.vanish, [], "nothing vanishes");
    assert.ok(step.plan.segments.length >= 1, "a plan always says what happens to the rows");
    for (const segment of step.plan.segments) assert.equal(segment.fall, 0, "nothing falls");
    assert.ok(
      step.plan.moves.every((move) => move.to === move.from),
      "every row stays exactly where it is",
    );
    assert.deepEqual(words(step.postClear), words(step.preClear), "preClear is already postClear");
    assert.equal(step.plan.moves.length, H);
  }
});

test("reconstruct is pure: it returns a fresh board and mutates nothing it was given", () => {
  const entry = FIXTURE.cases.find((one) => one.name.startsWith("game-seed"));
  assert.ok(entry, "a real game to play with");
  const board0 = Uint32Array.from(entry.board0);
  const timeline = buildTimeline(board0, entry.moves.map(word));

  // The caller's board is copied in, not aliased.
  board0[0] = 0xfffff;
  assert.deepEqual(words(timeline.board0), entry.board0, "the timeline owns its own board");

  for (const k of [0, 1, 3, timeline.steps.length]) {
    const first = reconstruct(timeline, k);
    const before = words(first);
    first[0] = 0xdead;
    first[4] = 0xbeef;
    const again = reconstruct(timeline, k);
    assert.deepEqual(words(again), before, `move ${k}: a mutated board is not remembered`);
    assert.notEqual(again, first, `move ${k}: every call hands back a new array`);
  }

  // A step's postClear is not something a renderer can damage by holding on to it:
  // every reconstruct hands back its own array, so writing into one changes nothing.
  const stepBoard = reconstruct(timeline, 1);
  assert.notEqual(stepBoard, timeline.steps[0].postClear, "no shared arrays");
  stepBoard.fill(0xfffff);
  assert.deepEqual(words(reconstruct(timeline, 1)), entry.frames[0], "history survives a write to a copy");
});

test("seeking backwards and forwards lands on the same boards as playing through", () => {
  for (const entry of FIXTURE.cases) {
    const timeline = buildTimeline(entry.board0, entry.moves.map(word));
    const forward = [];
    for (let k = 0; k <= entry.moves.length; k += 1) forward.push(words(reconstruct(timeline, k)));
    // The same seeks in a different order, as a scrub drag would issue them.
    for (let k = entry.moves.length; k >= 0; k -= 1) {
      assert.deepEqual(words(reconstruct(timeline, k)), forward[k], `${entry.name}: seek to ${k}`);
    }
    for (const k of [2, 0, 7, 1, 5]) {
      if (k > entry.moves.length) continue;
      assert.deepEqual(words(reconstruct(timeline, k)), forward[k], `${entry.name}: out-of-order seek`);
    }
  }
});

test("to seek is to rebuild: a timeline built from the Move list alone is the same timeline", () => {
  const entry = FIXTURE.cases.find((one) => one.name.startsWith("game-seed"));
  assert.ok(entry);
  const once = buildTimeline(entry.board0, entry.moves.map(word));
  const again = buildTimeline(entry.board0, entry.moves.map(word));
  for (let k = 0; k <= entry.moves.length; k += 1) {
    assert.deepEqual(words(reconstruct(again, k)), words(reconstruct(once, k)), `move ${k}`);
  }
});

test("the glyph table and the Sequence agree with the Move words", () => {
  for (const entry of FIXTURE.cases) {
    if (!entry.sequence) continue;
    const timeline = buildTimeline(entry.board0, entry.moves.map(word));
    for (const [index, step] of timeline.steps.entries()) {
      assert.equal(
        PIECE_LETTERS[step.piece],
        entry.sequence[index],
        `${entry.name}: move ${index + 1} names the Sequence's piece`,
      );
    }
  }
});

test("the spawn row is the box's top at row 0, and the drop is the engine's own", () => {
  for (const entry of FIXTURE.cases) {
    const timeline = buildTimeline(entry.board0, entry.moves.map(word));
    for (const [index, step] of timeline.steps.entries()) {
      const label = `${entry.name} move ${index + 1}`;
      const shape = rotationOf(step.piece, step.rotation);
      assert.equal(step.spawnShift, H - shape.height, `${label}: the box sits at the top row`);
      assert.equal(step.dropRows, step.spawnShift - step.restShift, `${label}: the fall is derived`);
      assert.ok(step.dropRows >= 0, `${label}: a piece never spawns below its landing`);
      assert.equal(step.spawnCells.length, 4, `${label}: four cells at spawn`);
      assert.equal(step.restCells.length, 4, `${label}: four cells at rest`);
      const cellCountAtRest = rowsOf(step.preClear).length - rowsOf(step.preBoard).length;
      assert.equal(cellCountAtRest, step.restCells.length, `${label}: the placement adds four cells`);
      for (const cell of step.spawnCells) {
        assert.ok(cell.row >= 0 && cell.row < H, `${label}: the spawn is on the board`);
        assert.ok(cell.col >= 0 && cell.col < W, `${label}: and inside a column`);
      }
      for (const cell of step.restCells) {
        assert.ok(cell.row >= 0 && cell.row < H, `${label}: the landing is on the board`);
      }
      // The spawn cells are the rest cells, shifted up by the drop.
      const spawnRows = step.spawnCells.map((cell) => cell.row).sort((a, b) => a - b);
      const restRows = step.restCells.map((cell) => cell.row).sort((a, b) => a - b);
      assert.deepEqual(
        spawnRows,
        restRows.map((row) => row - step.dropRows),
        `${label}: spawn == rest lifted by the drop distance`,
      );
    }
  }
});

test("unpackMove reads §4.1.1's word, and an impossible Move is a failure not a silent skip", () => {
  assert.deepEqual(unpackMove(word([3, 0, 8])), { piece: 3, rotation: 0, column: 8 });
  assert.deepEqual(unpackMove(word([6, 1, 2])), { piece: 6, rotation: 1, column: 2 });

  // A board whose columns are full to the top cannot admit a piece: the engine
  // reports that as no legal spawn, and so must the replay.
  const full = new Uint32Array(W).fill(0xfffff);
  assert.equal(restShift(fieldFrom(full), rotationOf(3, 0), 4), null, "no spawn on a full board");
  assert.throws(
    () => buildTimeline(full, [word([3, 0, 4])]),
    /move 1/,
    "the replay names the Move it cannot place",
  );
});

test("a step carries the engine's cells, not a re-derived approximation", () => {
  // The three-row and four-row clears have different heights of stack above them:
  // the plan says how far each surviving segment travels, and the cell counts add
  // up before and after.
  for (const name of ["four-row", "one-row", "mid-stack", "non-contiguous"]) {
    const entry = FIXTURE.cases.find((one) => one.name === name);
    const timeline = buildTimeline(entry.board0, entry.moves.map(word));
    const step = timeline.steps[0];
    assert.equal(
      cellCount(step.postClear),
      cellCount(step.preClear) - step.cleared.length * W,
      `${name}: a cleared row takes ten cells with it`,
    );
    const fallen = step.plan.moves.filter((move) => move.to !== move.from);
    assert.ok(fallen.length > 0, `${name}: something falls`);
  }
});
