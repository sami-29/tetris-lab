// The playback clock: time-based, pausable, seekable, and never the source of
// truth for the picture.
//
// `web/src/ui/clock.ts` holds `(k, into)` — which Move is on screen and how far
// into it — as a small pure state machine. There is no animation object and no
// "current animation" to cancel, which is exactly why a scrub is safe: the board
// is `reconstruct(k)` on the next frame, whatever happened before.
//
// These tests own the clock (no requestAnimationFrame, no browser) and assert the
// three properties the ticket turns on: k depends on elapsed time and not on the
// frame rate, pausing freezes the position without snapping it, and playing from a
// scrubbed Move continues from that Move's own board.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import {
  STALL_MS,
  initialState,
  pause,
  position,
  reanchor,
  restart,
  resume,
  seek,
  setSpeed,
  stepAt,
  toggle,
} from "../src/ui/clock.ts";
import { buildTimeline, reconstruct } from "../src/ui/timeline.ts";
import { msPerMove } from "../src/ui/phases.ts";

const FIXTURE = JSON.parse(
  readFileSync(fileURLToPath(new URL("./fixtures/replay-vectors.json", import.meta.url)), "utf8"),
);

const word = ([piece, rotation, column]) => ((piece << 16) | (rotation << 8) | column) >>> 0;
const words = (field) => [...field];

function gameTimeline(name = "game-seed0") {
  const entry = FIXTURE.cases.find((one) => one.name === name);
  assert.ok(entry, `the fixture has the ${name} case`);
  return { entry, timeline: buildTimeline(entry.board0, entry.moves.map(word)) };
}

/** A state that is playing from the starting Board at `mps`. */
function playingFrom(timeline, mps) {
  return resume(timeline, initialState(mps, 0), 0);
}

/** Play a cadence of frames up to `until`, re-anchoring like the page does. */
function playCadence(timeline, state, until, frameMs) {
  let current = state;
  for (let now = state.t + frameMs; now <= until + 1e-9; now += frameMs) {
    current = reanchor(timeline, current, now);
  }
  return current;
}

test("k depends on elapsed time, not on the frame rate", () => {
  const { timeline } = gameTimeline();
  const stepMs = msPerMove(4);
  const until = 3000;

  // Three cadences over the same 3000 ms: they must agree on k, and on how far
  // into that step they are (to floating point).
  const results = [1000 / 120, 1000 / 60, 1000 / 30].map((frameMs) => {
    const state = playCadence(timeline, playingFrom(timeline, 4), until, frameMs);
    return position(timeline, state, until);
  });
  for (const result of results) {
    assert.equal(result.k, Math.floor(until / stepMs), "k is elapsed time over step time");
    assert.ok(Math.abs(result.into - (until % stepMs)) < 1e-6, "and the position inside the step");
  }
  assert.deepEqual(
    results.map((result) => result.k),
    [results[0].k, results[0].k, results[0].k],
    "all three cadences land on the same Move",
  );

  // A fast speed must not skip Moves at a normal frame rate: at 16 mps a step is
  // 62.5 ms and a 60 Hz frame is 16.7 ms, so at most one Move per frame.
  const fast = buildTimeline(
    timeline.board0,
    timeline.moves.map((move) => word([move.piece, move.rotation, move.column])),
  );
  let state = playingFrom(fast, 16);
  let previous = position(fast, state, 0).k;
  for (let now = 1000 / 60; now <= 600; now += 1000 / 60) {
    state = reanchor(fast, state, now);
    const current = position(fast, state, now).k;
    assert.ok(current - previous <= 1, `no Move is skipped at 60 Hz (${now} ms)`);
    previous = current;
  }
  assert.ok(previous > 0, "and the clock actually advanced");
});

test("a frame longer than a step samples the timeline later instead of tearing", () => {
  const { timeline } = gameTimeline();
  const mps = 16;
  const stepMs = msPerMove(mps);
  let state = playingFrom(timeline, mps);
  // A 100 ms frame over a 62.5 ms step: the picture is always the exact state of
  // the step in progress, at a later sample.
  for (let now = 100; now <= 2000; now += 100) {
    state = reanchor(timeline, state, now);
    const at = position(timeline, state, now);
    assert.equal(at.k, Math.min(timeline.steps.length, Math.floor(now / stepMs)), "k is still time/step");
    assert.ok(at.into >= 0 && at.into < stepMs + 1e-9, "and the position is inside one step");
  }
});

test("the clock stops at the last Move instead of running off the end", () => {
  const { timeline } = gameTimeline();
  const total = timeline.steps.length;
  const state = playCadence(timeline, playingFrom(timeline, 4), 30_000, 50);
  const at = position(timeline, state, 30_000);
  assert.equal(at.k, total, "the last position is the last Move");
  assert.equal(at.into, 0, "with nothing left to play into");
  assert.equal(at.atEnd, true, "and it says so, because the button becomes a restart");
  assert.equal(stepAt(timeline, total), timeline.steps[total - 1], "the last step is reachable");
  assert.equal(stepAt(timeline, 0), null, "move 0 is the starting Board, not a step");
  assert.equal(stepAt(timeline, total + 3), null, "and past the end there is nothing");
});

test("pause freezes the position without snapping it, and resume continues from the freeze", () => {
  const { timeline } = gameTimeline();
  const mps = 1.43;
  const stepMs = msPerMove(mps);

  const playing = reanchor(timeline, playingFrom(timeline, mps), 200);
  const before = position(timeline, playing, 200);
  assert.ok(before.into > 0, "the first Move is in flight");

  const paused = pause(timeline, playing, 200);
  assert.equal(paused.playing, false);
  assert.equal(paused.k, before.k, "pause keeps the Move");
  assert.ok(Math.abs(paused.into - before.into) < 1e-9, "and the position inside it");

  // A minute later the frozen pose is exactly the same pose.
  const later = position(timeline, paused, 200 + 60_000);
  assert.equal(later.k, before.k, "a paused clock does not advance");
  assert.ok(Math.abs(later.into - before.into) < 1e-9, "and does not drift");

  const resumeAt = 200 + 60_000;
  const resumed = resume(timeline, paused, resumeAt);
  const played = playCadence(timeline, resumed, resumeAt + 600, 100);
  const after = position(timeline, played, resumeAt + 600);
  const expected = before.into + 600;
  assert.equal(
    after.k,
    before.k + Math.floor(expected / stepMs),
    "the drop continues where it froze, not from the start of the Move",
  );
  assert.ok(Math.abs(after.into - (expected % stepMs)) < 1e-6, "at the same fraction of the step");

  // Toggling is pause then resume, with no other difference.
  const toggled = toggle(timeline, toggle(timeline, playing, 200), 200);
  assert.equal(toggled.playing, true);
  assert.ok(Math.abs(position(timeline, toggled, 200).into - before.into) < 1e-9);
});

test("changing speed keeps the fraction of the Move that is already played", () => {
  const { timeline } = gameTimeline();
  const start = reanchor(timeline, playingFrom(timeline, 1.43), 200);
  const before = position(timeline, start, 200);
  const fraction = before.into / msPerMove(1.43);

  const faster = setSpeed(timeline, start, 4, 200);
  assert.equal(faster.mps, 4, "the speed changed");
  assert.equal(faster.k, before.k, "but not the Move on screen");
  assert.ok(
    Math.abs(faster.into / msPerMove(4) - fraction) < 1e-9,
    "the falling piece stays where it was in its fall instead of teleporting",
  );

  // The same holds while paused: the frozen pose survives a speed change.
  const paused = pause(timeline, start, 200);
  const slowed = setSpeed(timeline, paused, 0.25, 200);
  assert.equal(slowed.playing, false, "a speed change does not start playback");
  assert.ok(Math.abs(slowed.into / msPerMove(0.25) - fraction) < 1e-9, "the frozen fraction is kept");

  // And the speed is clamped to the page's range, not to whatever a caller passes.
  assert.equal(setSpeed(timeline, start, 1000, 200).mps, 40);
  assert.equal(setSpeed(timeline, start, 0, 200).mps, 0.25);
});

test("playing from a scrubbed Move continues from that Move's own board", () => {
  const { timeline } = gameTimeline();
  const total = timeline.steps.length;

  for (const k of [0, 1, 7, total - 1, total]) {
    const state = initialState(4, 0);
    const scrubbed = seek(timeline, state, k, 1000);
    assert.equal(scrubbed.playing, false, "a scrub pauses playback");
    assert.equal(scrubbed.k, k, "and lands exactly on the Move asked for");
    assert.equal(scrubbed.into, 0, "at the start of it");

    // The board on screen is the search's board at move k…
    assert.deepEqual(words(reconstruct(timeline, k)), k === 0 ? words(timeline.board0) : words(timeline.steps[k - 1].postClear));

    if (k === total) continue;
    // …and the next step is applied *to that board*, not to a stale one.
    const next = stepAt(timeline, k + 1);
    assert.deepEqual(words(next.preBoard), words(reconstruct(timeline, k)), `move ${k + 1} starts here`);
    assert.deepEqual(words(next.postClear), words(reconstruct(timeline, k + 1)), `and ends here`);

    const resumed = resume(timeline, scrubbed, 2000);
    const halfway = position(timeline, resumed, 2000 + 100);
    assert.equal(halfway.k, k, "resuming from a scrub plays the Move that was asked for");
    assert.ok(halfway.into > 0, "and advances inside it");
  }

  // Out-of-range seeks clamp instead of throwing: the seek bar cannot overshoot.
  assert.equal(seek(timeline, initialState(4, 0), -5, 0).k, 0);
  assert.equal(seek(timeline, initialState(4, 0), total + 9, 0).k, total);
});

test("the stall guard clamps a hidden tab and does not replay the missing minute", () => {
  const { timeline } = gameTimeline();
  const mps = 4;
  const stepMs = msPerMove(mps);
  const state = playingFrom(timeline, mps);

  const at = position(timeline, state, 60_000);
  const most = Math.ceil(STALL_MS / stepMs);
  assert.ok(at.k <= most, `a minute away advances at most ${most} Moves, not ${at.k}`);

  // Re-anchoring drops the excess for good rather than banking it, so the page
  // resumes at a human pace instead of fast-forwarding to the end.
  const steady = reanchor(timeline, state, 60_000);
  assert.equal(steady.k, at.k, "the re-anchored state matches the reported position");
  assert.equal(steady.t, 60_000, "and is anchored at now");
  const next = reanchor(timeline, steady, 60_000 + 250);
  assert.ok(next.k - steady.k <= most, "the next quarter second advances at the normal rate");
});

test("restart goes back to move 0, playing, at the speed the visitor chose", () => {
  const { timeline } = gameTimeline();
  const playing = setSpeed(timeline, playCadence(timeline, playingFrom(timeline, 1.43), 5_000, 50), 2, 5_000);
  const at = position(timeline, playing, 5_000);
  assert.ok(at.k > 0, "we are somewhere in the game");

  const again = restart(timeline, playing, 5_000);
  assert.equal(again.k, 0, "back to the starting Board");
  assert.equal(again.into, 0, "with no Move in flight");
  assert.equal(again.playing, true, "and playing");
  assert.equal(again.mps, 2, "at the same speed");
  assert.deepEqual(words(reconstruct(timeline, 0)), words(timeline.board0), "which is board 0");
});

test("the clock never writes to the timeline", () => {
  const { timeline } = gameTimeline();
  const before = timeline.steps.map((step) => words(step.postClear));
  let state = initialState(2, 0);
  for (let now = 0; now < 4000; now += 50) state = reanchor(timeline, state, now);
  pause(timeline, state, 4000);
  seek(timeline, state, 3, 4000);
  setSpeed(timeline, state, 8, 4000);
  restart(timeline, state, 4000);
  for (const [index, step] of timeline.steps.entries()) {
    assert.deepEqual(words(step.postClear), before[index], `step ${index + 1} is untouched`);
  }
});
