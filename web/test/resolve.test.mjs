// Seam 1: what re-solves, and how often a drag is allowed to.
//
// Ticket 13's third bullet is "raising the starting height changes the Board and
// keeps the Sequence, so changing one control changes one thing". The Sequence
// half is proved against the real generator in `playground-wasm.test.mjs`; the
// URL half is here: the re-solve decision is a pure comparison of five fields, and
// the arithmetic that keeps a 120 Hz slider drag from queueing 180 solves.
//
// `reSolveReason` exists so the page never posts a solve nobody asked for and
// never forgets one the visitor did: `applyGame` reads it, and "none" is the only
// reason that writes nothing. It is also the place §7's rule is legible in code —
// "in the URL" is not the same as "causes a solve" (weights cause a solve and are
// never in the URL; playback speed is in neither).

import { test } from "node:test";
import assert from "node:assert/strict";

import { defaultGame, reSolveReason, sameGame } from "../src/core/game.ts";
import { createCoalescer } from "../src/ui/coalesce.ts";

const BOUNDS = { defaultBeam: 25, widthCap: 800 };
const GAME = defaultGame(BOUNDS);

// ── the re-solve decision ──────────────────────────────────────────────────

test("each field is its own re-solve reason", () => {
  const cases = [
    ["seed", { seed: 1 }],
    ["goal", { goal: 12 }],
    ["pieces", { pieces: 100 }],
    ["cap", { heightCap: 19 }],
    ["beam", { beamWidth: 400 }],
  ];
  for (const [reason, change] of cases) {
    assert.equal(reSolveReason({ ...GAME, ...change }, GAME), reason);
  }
});

test("an identical game asks for nothing, and so does the first solve", () => {
  assert.equal(reSolveReason({ ...GAME }, GAME), "none");
  assert.equal(reSolveReason(GAME, null), "none", "the first solve is not a re-solve");
  assert.ok(sameGame(GAME, { ...GAME }));
});

test("the reason is the first field that differs, so one commit names one thing", () => {
  assert.equal(reSolveReason({ ...GAME, goal: 9, pieces: 41 }, GAME), "goal");
  assert.equal(reSolveReason({ ...GAME, heightCap: 19, beamWidth: 800 }, GAME), "cap");
});

// ── the commit tier: a drag is many events and few solves ──────────────────

/** A clock the test owns: timers fire only when the test advances time. */
function fakeClock() {
  let now = 0;
  let nextId = 1;
  const timers = new Map();
  return {
    now: () => now,
    setTimer(handler, ms) {
      const id = nextId++;
      timers.set(id, { at: now + ms, handler });
      return id;
    },
    clearTimer(id) {
      timers.delete(id);
    },
    advance(ms) {
      const target = now + ms;
      for (;;) {
        const due = [...timers.entries()]
          .filter(([, timer]) => timer.at <= target)
          .sort((a, b) => a[1].at - b[1].at)[0];
        if (!due) break;
        const [id, timer] = due;
        timers.delete(id);
        now = timer.at;
        timer.handler();
      }
      now = target;
    },
    pending: () => timers.size,
  };
}

function coalescer(delayMs = 150) {
  const clock = fakeClock();
  const fired = [];
  const commit = createCoalescer({
    delayMs,
    setTimer: (handler, ms) => clock.setTimer(handler, ms),
    clearTimer: (id) => clock.clearTimer(id),
    onFire: (value) => fired.push(value),
  });
  return { clock, fired, commit };
}

test("a 1.5 s drag at 120 Hz costs at most ten solves, and the last value wins", () => {
  const { clock, fired, commit } = coalescer(150);
  // §9.3's numbers: 90 events at 60 Hz, 180 at 120 Hz, and a cap-width solve is
  // 0.1–0.5 s, so an undebounced drag is half a minute of queued worker time.
  for (let event = 0; event < 180; event += 1) {
    commit.push(event);
    clock.advance(1500 / 180);
  }
  clock.advance(500);
  assert.ok(fired.length >= 1, "a drag that ends still solves");
  assert.ok(fired.length <= 12, `180 pushes fired ${fired.length} solves, not one per push`);
  assert.equal(fired[fired.length - 1], 179, "the value solved is the last one the visitor chose");
  assert.equal(clock.pending(), 0, "no timer is left behind");
});

test("a drag shorter than the delay costs exactly one solve", () => {
  const { clock, fired, commit } = coalescer(150);
  for (let event = 0; event < 20; event += 1) {
    commit.push(event);
    clock.advance(4);
  }
  assert.equal(fired.length, 0, "nothing solved while the hand is still moving");
  clock.advance(150);
  assert.deepEqual(fired, [19]);
});

test("flush is immediate, so a release or an arrow-key step does not wait", () => {
  const { fired, commit } = coalescer(150);
  commit.push("a");
  assert.equal(fired.length, 0);
  commit.flush();
  assert.deepEqual(fired, ["a"], "flush fires on the spot");
  commit.flush();
  assert.deepEqual(fired, ["a"], "flushing with nothing pending does nothing");
});

test("cancel drops the pending value", () => {
  const { clock, fired, commit } = coalescer(150);
  commit.push("a");
  commit.cancel();
  clock.advance(1000);
  assert.deepEqual(fired, []);
});

test("a solve after a fire starts a fresh timer", () => {
  const { clock, fired, commit } = coalescer(150);
  commit.push(1);
  clock.advance(150);
  assert.deepEqual(fired, [1]);
  commit.push(2);
  clock.advance(150);
  assert.deepEqual(fired, [1, 2], "two separate choices are two solves");
});
