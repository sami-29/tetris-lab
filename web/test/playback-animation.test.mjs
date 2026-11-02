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
