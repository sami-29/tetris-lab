// Playback, driven by a fake clock: the animation's landing positions must be the
// engine's.
//
// `web/test/playback.test.mjs` proves the *field* replays a Move list correctly.
// This file proves the thing the page actually runs: `Playback` walks the Move
// words, drops each piece, stamps it where it lands and clears what it completed —
// and when it finishes, the field it is showing is the field the engine's Move list
// describes. It is checked with a stub render target and an owned clock, so no
// browser, no canvas and no timing flake is involved.

import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { loadEngineFromBytes } from "../src/core/loader.ts";
import { Playback } from "../src/ui/playback.ts";
import { clearFullRows, fieldFrom, place, restShift } from "../src/ui/field.ts";
import { rotationOf } from "../src/ui/pieces.ts";

const ARTIFACT = fileURLToPath(new URL("../pkg/engine.wasm", import.meta.url));

function wasmBytes() {
  if (!existsSync(ARTIFACT)) {
    throw new Error(`missing ${ARTIFACT} — run \`node scripts/build-wasm.mjs\` first`);
  }
  return new Uint8Array(readFileSync(ARTIFACT));
}

/** A clock the test owns, so a 25-second animation runs in microseconds. */
function fakeClock() {
  let now = 0;
  let queued = null;
  return {
    scheduler: {
      frame(callback) {
        queued = callback;
        return 1;
      },
      cancel() {
        queued = null;
      },
      now: () => now,
    },
    /** Run frames until the animation stops asking for them, or `steps` run out. */
    run(steps = 4000, stepMs = 16) {
      for (let i = 0; i < steps && queued; i += 1) {
        now += stepMs;
        const callback = queued;
        queued = null;
        callback(now);
      }
      return now;
    },
    /** One frame of `stepMs`: the shape a playback test wants to sample with. */
    advance(stepMs = 16) {
      now += stepMs;
      const callback = queued;
      queued = null;
      if (callback) callback(now);
      return now;
    },
    now: () => now,
    stop() {
      queued = null;
    },
  };
}

/** The same Move list played through the field primitives, as the engine means it. */
function replay(boardWords, moveWords) {
  const field = fieldFrom(boardWords);
  let lines = 0;
  for (const word of moveWords) {
    const piece = (word >>> 16) & 0xff;
    const rotation = (word >>> 8) & 0xff;
    const column = word & 0xff;
    const shape = rotationOf(piece, rotation);
    assert.ok(shape, "the move names a rotation the glyph table has");
    const shift = restShift(field, shape, column);
    assert.notEqual(shift, null, "the move is spawn-legal on the field it replays onto");
    place(field, shape, column, shift);
    lines += clearFullRows(field);
  }
  return { field, lines };
}

function assertSameField(actual, expected, message) {
  assert.deepEqual([...actual], [...expected], message);
}

test("playback ends on the field the engine's Move list describes", async () => {
  const engine = await loadEngineFromBytes(wasmBytes());

  for (const [seed, goal] of [
    [0, 8],
    [42, 8],
    [7, 8],
  ]) {
    assert.equal(engine.genGame(seed, 40, 7), 0, `seed ${seed} generates`);
    const board = Uint32Array.from(engine.board());
    const outcome = engine.solve(goal, 25);
    assert.equal(outcome.status, 0);
    assert.ok(outcome.won, `seed ${seed} wins at width 25`);
    const moves = Uint32Array.from(engine.moves(outcome.moveCount));

    const clock = fakeClock();
    const drawn = [];
    let finished = null;
    let awaitingFinalField = false;
    let finalField = null;
    const playback = new Playback(
      {
        draw(field) {
          const copy = Uint32Array.from(field);
          drawn.push(copy);
          // The list is complete; the next thing drawn is the settled board.
          if (awaitingFinalField) {
            finalField = copy;
            awaitingFinalField = false;
          }
        },
      },
      {
        type: "game",
        requestId: 1,
        input: { seed, goal, pieces: 40, heightCap: 7, beamWidth: 25 },
        board,
        sequence: engine.sequence(40),
        outcome,
        moves,
        elapsedMs: 0,
      },
      {
        scheduler: clock.scheduler,
        onStatus(status) {
          if (finished === null && status.played === status.total && status.total > 0) {
            finished = { ...status };
            awaitingFinalField = true;
          }
        },
      },
    );

    playback.start();
    clock.run();
    playback.stop();

    assert.ok(finished, `seed ${seed}: playback reached the end of the Move list`);
    assert.equal(finished.lines, outcome.lines, `seed ${seed}: playback cleared the engine's Lines`);
    assert.ok(drawn.length > 0, `seed ${seed}: playback drew`);

    const expected = replay(board, moves);
    assert.ok(finalField, `seed ${seed}: playback drew the settled board after the last move`);
    assertSameField(
      finalField,
      expected.field,
      `seed ${seed}: the settled field is the engine's Move list played out`,
    );

    // A landing is a landing, not a stamp at the spawn row: the settled board must
    // sit lower than the spawn rows for the pieces that were played. (`restShift`
    // encodes gravity, so the equality above already implies this; the check names
    // the fault that a stamp-at-spawn would produce.)
    const settled = [...finalField].reduce((sum, word) => sum + (word & 0xffff), 0);
    assert.ok(settled > 0, `seed ${seed}: the settled board has cells in the well`);
  }
});

test("a piece in flight is drawn above its landing row and falls into it", async () => {
  const engine = await loadEngineFromBytes(wasmBytes());
  assert.equal(engine.genGame(0, 40, 7), 0);
  const board = Uint32Array.from(engine.board());
  const outcome = engine.solve(8, 25);
  const moves = Uint32Array.from(engine.moves(outcome.moveCount));

  const frames = [];
  const clock = fakeClock();
  const playback = new Playback(
    {
      draw(field, overlay) {
        frames.push({ field: Uint32Array.from(field), overlay: overlay ?? {} });
      },
    },
    {
      type: "game",
      requestId: 1,
      input: { seed: 0, goal: 8, pieces: 40, heightCap: 7, beamWidth: 25 },
      board,
      sequence: engine.sequence(40),
      outcome,
      moves,
      elapsedMs: 0,
    },
    { scheduler: clock.scheduler },
  );

  playback.start();
  clock.run(40); // a few frames: the first piece is on its way down
  playback.stop();

  const first = frames[0];
  assert.ok(first.overlay.piece && first.overlay.piece.length === 4, "the first frame shows a piece");
  assert.ok(first.overlay.ghost && first.overlay.ghost.length === 4, "and where it will land");

  const pieceRows = first.overlay.piece.map((cell) => cell.row);
  const ghostRows = first.overlay.ghost.map((cell) => cell.row);
  const minPiece = Math.min(...pieceRows);
  const maxGhost = Math.max(...ghostRows);
  assert.ok(minPiece < maxGhost, "the piece starts above its landing row");
  assert.ok(
    pieceRows.every((row) => row >= 0 && row < 20),
    "the piece is on the board while it falls",
  );

  // The field the piece is falling onto is untouched: a falling piece is drawn as an
  // overlay, not stamped into the board.
  assert.deepEqual([...first.field], [...board], "the settled field is the starting Board");
});

// ---- Ticket 10: drops that land, line clears that collapse, seeks that hold ----
//
// The two tests above prove the model. These prove the *animation*: the frames the
// page puts on the canvas between one board and the next. A drop has to leave the
// spawn row and arrive at the engine's resting shift, and a clear has to be a
// collapse — the cleared rows contracting away while the stack above them falls
// into the gap, each surviving run by its own distance — rather than the
// translucent flash the prototype shipped.

import { buildTimeline, reconstruct } from "../src/ui/timeline.ts";
import { msPerMove, phases } from "../src/ui/phases.ts";

const FIXTURE = JSON.parse(
  readFileSync(fileURLToPath(new URL("./fixtures/replay-vectors.json", import.meta.url)), "utf8"),
);

const fixtureWord = ([piece, rotation, column]) => ((piece << 16) | (rotation << 8) | column) >>> 0;

function fixtureGame(name) {
  const entry = FIXTURE.cases.find((one) => one.name === name);
  assert.ok(entry, `the fixture has the ${name} case`);
  return {
    entry,
    game: {
      type: "game",
      requestId: 1,
      input: { seed: entry.seed ?? 0, goal: 8, pieces: 40, heightCap: 7, beamWidth: 25 },
      board: Uint32Array.from(entry.board0),
      sequence: entry.sequence ?? "IJLOSTZ".repeat(30).slice(0, entry.moves.length),
      outcome: {
        status: 0,
        won: true,
        lines: entry.lines.at(-1) ?? 0,
        evaluations: 0,
        moveCount: entry.moves.length,
      },
      moves: Uint32Array.from(entry.moves.map(fixtureWord)),
      elapsedMs: 0,
    },
  };
}

/** Drive a Playback and collect every frame it draws. */
function record(game, { speed = 1, frames = 2000, stepMs = 5, start = true } = {}) {
  const drawn = [];
  const clock = fakeClock();
  const playback = new Playback(
    {
      draw(field, overlay) {
        drawn.push({
          field: Uint32Array.from(field),
          piece: (overlay?.piece ?? []).map((cell) => ({ ...cell })),
          ghost: (overlay?.ghost ?? []).map((cell) => ({ ...cell })),
          cells: (overlay?.cells ?? []).map((cell) => ({ ...cell })),
          bands: (overlay?.bands ?? []).map((band) => ({ ...band })),
          now: clock.now(),
        });
      },
    },
    game,
    { scheduler: clock.scheduler, speed },
  );
  if (start) playback.play();
  for (let index = 0; index < frames; index += 1) clock.advance(stepMs);
  return { playback, drawn, clock };
}

const rowsFor = (field) => {
  const rows = [];
  for (let col = 0; col < 10; col += 1) {
    for (let bit = 0; bit < 20; bit += 1) if ((field[col] & (1 << bit)) !== 0) rows.push({ col, row: 19 - bit });
  }
  return rows;
};
const key = (cell) => `${cell.col}:${Math.round(cell.row)}`;
const keys = (cells) => cells.map(key).sort();

test("a drop leaves the spawn row and lands on the engine's shift", () => {
  const { entry, game } = fixtureGame("four-row");
  const timeline = buildTimeline(entry.board0, entry.moves.map(fixtureWord));
  const step = timeline.steps[0];
  assert.ok(step.dropRows > 1, "the four-row case is a real fall");

  const { drawn } = record(game, { speed: 1, frames: 400, stepMs: 10 });
  const falling = drawn.filter((frame) => frame.piece.length === 4);
  assert.ok(falling.length >= 4, "the drop is drawn over several frames, not stamped");

  // The first frame is the piece at its spawn row, not a few rows above the rest
  // position: a 16-row fall is drawn as a 16-row fall.
  assert.deepEqual(
    falling[0].piece.map((cell) => Math.round(cell.row)).sort((a, b) => a - b),
    step.spawnCells.map((cell) => cell.row).sort((a, b) => a - b),
    "the piece starts on the spawn row",
  );
  assert.deepEqual(
    falling[0].ghost.map((cell) => Math.round(cell.row)).sort((a, b) => a - b),
    step.restCells.map((cell) => cell.row).sort((a, b) => a - b),
    "and the ghost shows where the engine will put it",
  );

  // The fall is monotone: no cell of the piece ever rises, and a drop never
  // changes column — it is gravity, not a different placement.
  for (let index = 1; index < falling.length; index += 1) {
    for (const [cellIndex, cell] of falling[index].piece.entries()) {
      const previous = falling[index - 1].piece[cellIndex];
      assert.ok(cell.row >= previous.row - 1e-9, `frame ${index}: the piece only falls`);
      assert.equal(cell.col, previous.col, "a drop does not change column");
    }
  }
  const travelled = Math.abs(
    Math.max(...falling.at(-1).piece.map((cell) => cell.row)) -
      Math.max(...falling[0].piece.map((cell) => cell.row)),
  );
  assert.ok(travelled > 1, `the piece travelled ${travelled} rows on screen`);

  // It lands: some frame draws the piece at the engine's resting cells, and the
  // board it was falling onto was never this Move's board.
  assert.ok(
    drawn.some(
      (frame) =>
        frame.piece.length === 4 && keys(frame.piece).join("|") === keys(step.restCells).join("|"),
    ),
    "a frame draws the piece exactly at rest",
  );
  for (const frame of falling) {
    assert.deepEqual([...frame.field], [...step.preBoard], "the falling piece is an overlay, not a stamp");
  }
});

test("a line clear is a collapse: the bands contract and the stack falls into the gap", () => {
  const { entry, game } = fixtureGame("non-contiguous");
  const timeline = buildTimeline(entry.board0, entry.moves.map(fixtureWord));
  const step = timeline.steps[0];
  assert.deepEqual(step.cleared, [14, 17]);

  const plan = phases(msPerMove(1), 2, 1000 / 60);
  const { drawn } = record(game, { speed: 1, frames: 400, stepMs: 5 });
  const collapsing = drawn.filter((frame) => frame.bands.length > 0);
  assert.ok(collapsing.length >= 5, "the clear is drawn over several frames, not flashed");

  // The cleared rows are the bands, and they contract away rather than fade.
  for (const frame of collapsing) {
    assert.deepEqual(frame.bands.map((band) => band.row), [14, 17], "both cleared rows, in order");
    for (const band of frame.bands) {
      assert.ok(band.squeeze > 0 && band.squeeze <= 1, "a band squeezes from full height");
    }
  }
  const squeezes = collapsing.map((frame) => frame.bands[0].squeeze);
  for (let index = 1; index < squeezes.length; index += 1) {
    assert.ok(squeezes[index] <= squeezes[index - 1] + 1e-9, "the bands only ever contract");
  }
  assert.ok(squeezes.at(-1) < 0.5, "and by the end of the collapse they are nearly gone");

  // The surviving cells are carried as moving cells, not left on the board: every
  // cell of the board is in the overlay, so nothing can show through the gap.
  const postCells = rowsFor(step.postClear);
  for (const frame of collapsing) {
    assert.equal(frame.cells.length, postCells.length, "every surviving cell is drawn");
  }

  // The last frame of the collapse is the board the engine produced…
  assert.deepEqual(keys(collapsing.at(-1).cells), keys(postCells), "the collapse ends on postClear");

  // …and the parts of the stack fall different distances: the rows below the lowest
  // cleared row do not move at all, while rows above one clear but below another
  // fall one row. One band moving as a unit cannot produce that.
  const first = collapsing[0].cells;
  const last = collapsing.at(-1).cells;
  const displacements = new Set();
  for (const [index, cell] of first.entries()) {
    displacements.add(Math.round(last[index].row) - Math.round(cell.row));
  }
  assert.deepEqual([...displacements].sort((a, b) => a - b), [0, 1], "two different fall distances");
  assert.equal(plan.mode, "full", "at this speed the clear is animated in full");

  // Finally: the collapse lands on the Move's own board.
  const after = drawn.filter((frame) => frame.bands.length === 0 && frame.piece.length === 0);
  assert.ok(
    after.some((frame) => [...frame.field].join(",") === [...step.postClear].join(",")),
    "the board after the collapse is the engine's board after the Move",
  );
});

test("the rows under a clear are drawn where they were, before and after", () => {
  const { entry, game } = fixtureGame("mid-stack");
  const timeline = buildTimeline(entry.board0, entry.moves.map(fixtureWord));
  const step = timeline.steps[0];
  const lowest = Math.max(...step.cleared);

  const { drawn } = record(game, { speed: 1, frames: 300, stepMs: 5 });
  const collapsing = drawn.filter((frame) => frame.bands.length > 0);
  assert.ok(collapsing.length > 0, "the clear is animated");

  // Rows that sit below every cleared row are drawn at the same place for the
  // whole collapse: nothing about them is interpolated, because nothing moves.
  const below = collapsing.map((frame) =>
    frame.cells
      .filter((cell) => cell.row > lowest)
      .map((cell) => key(cell))
      .join("|"),
  );
  assert.ok(below[0].length > 0, "the case has cells below the clear");
  assert.equal(new Set(below).size, 1, "one geometry below the clear, for the whole collapse");

  // And the rows above it do move, by exactly one row: a clear that draws nothing
  // moving would fail here.
  const above = (frame) =>
    frame.cells
      .filter((cell) => cell.row < lowest)
      .map((cell) => Math.round(cell.row) * 100 + cell.col)
      .sort((a, b) => a - b);
  assert.notDeepEqual(above(collapsing[0]), above(collapsing.at(-1)), "the stack above fell");
});

test("seek then play: playback continues from the scrubbed Move's own board", () => {
  const { entry, game } = fixtureGame("game-seed0");
  const timeline = buildTimeline(entry.board0, entry.moves.map(fixtureWord));
  const target = 12;
  const next = timeline.steps[target];

  const drawn = [];
  const clock = fakeClock();
  const playback = new Playback(
    {
      draw(field, overlay) {
        drawn.push({
          field: Uint32Array.from(field),
          piece: (overlay?.piece ?? []).map((cell) => ({ ...cell })),
          ghost: (overlay?.ghost ?? []).map((cell) => ({ ...cell })),
        });
      },
    },
    game,
    { scheduler: clock.scheduler, speed: 1 },
  );

  playback.seek(target);
  assert.equal(playback.k, target, "the scrub lands on the Move asked for");
  assert.equal(playback.playing, false, "and pauses");
  assert.deepEqual(
    [...drawn.at(-1).field],
    [...reconstruct(timeline, target)],
    "the posed board is the engine's board at that Move",
  );

  playback.play();
  for (let index = 0; index < 40; index += 1) clock.advance(10);
  const playing = drawn.slice(-40);
  assert.ok(playing.length > 0, "playback drew after the scrub");
  let sawDrop = false;
  for (const frame of playing) {
    if (frame.piece.length === 0) continue;
    sawDrop = true;
    assert.deepEqual([...frame.field], [...next.preBoard], "the drop starts from the scrubbed board");
    assert.deepEqual(
      frame.ghost.map((cell) => `${cell.col}:${Math.round(cell.row)}`).sort(),
      next.restCells.map((cell) => `${cell.col}:${cell.row}`).sort(),
      "onto the Move that follows the scrub",
    );
    break;
  }
  assert.ok(sawDrop, "the Move after a seek is drawn as a drop");

  // And the Move after a seek is applied to that board, not to a stale one.
  assert.deepEqual([...reconstruct(timeline, target)], [...next.preBoard]);
  assert.deepEqual([...reconstruct(timeline, target + 1)], [...next.postClear]);
});

test("pause, resume and restart are states, not one-shot flags", () => {
  const { game } = fixtureGame("game-seed0");
  const drawn = [];
  const clock = fakeClock();
  const playback = new Playback(
    {
      draw(field) {
        drawn.push(Uint32Array.from(field));
      },
    },
    game,
    { scheduler: clock.scheduler, speed: 2 },
  );

  playback.play();
  for (let index = 0; index < 30; index += 1) clock.advance(10);
  const reached = playback.k;
  assert.ok(reached >= 0, "the clock is running");

  playback.pause();
  const pausedFrames = drawn.length;
  for (let index = 0; index < 30; index += 1) clock.advance(10);
  assert.equal(playback.k, reached, "a paused playback does not advance");
  assert.equal(playback.playing, false);

  playback.play();
  for (let index = 0; index < 60; index += 1) clock.advance(10);
  assert.ok(playback.k >= reached, "resuming continues from where it paused");
  assert.ok(drawn.length > pausedFrames, "and keeps drawing");

  playback.restart();
  assert.equal(playback.k, 0, "restart is the starting Board");
  assert.equal(playback.playing, true, "and it plays");
  assert.equal(playback.speed, 2, "at the speed the visitor chose");

  playback.setSpeed(8);
  assert.equal(playback.speed, 8, "the speed is settable at any time");
  playback.stop();
});
