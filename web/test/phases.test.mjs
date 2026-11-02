// Playback speed and the phase budget: continuous, time-based, and never
// overspent.
//
// `web/src/ui/phases.ts` owns two decisions and nothing else: what a slider
// position means in Moves per second, and how one Move's time is divided between a
// drop, a line-clear collapse and a beat of rest. Both are pure arithmetic, which
// is why they are tested here rather than judged by eye in a browser.
//
// The rules the assertions below pin down are the ticket's: speed is *continuous*
// (a log-mapped slider, not four multipliers), a clear costs more time when it
// removes more rows but not proportionally more (the gravity rule, t ∝ √d), and no
// phase plan ever spends more time than the Move has. The phases are shares of the
// step rather than a table of milliseconds per preset, which is the difference
// between the same animation faster and a different animation at 4×.

import { test } from "node:test";
import assert from "node:assert/strict";

import { MPS_DEFAULT, MPS_MAX, MPS_MIN, mpsToSlider, msPerMove, phases, sliderToMps } from "../src/ui/phases.ts";

const FRAME_MS = 1000 / 60;

test("speed is a real number of Moves per second, with no preset in sight", () => {
  assert.deepEqual([MPS_MIN, MPS_MAX, MPS_DEFAULT], [0.25, 40, 1.43]);
  assert.ok(Math.abs(msPerMove(MPS_DEFAULT) - 699.3) < 0.1, "the default is the prototype's pace");
  assert.equal(msPerMove(2), 500, "two Moves a second is half a second each");

  // Continuous: a thousand samples between the ends, each its own speed, rather
  // than four multipliers with nothing in between.
  const speeds = new Set();
  for (let index = 0; index <= 1000; index += 1) speeds.add(sliderToMps(index / 1000).toFixed(6));
  assert.equal(speeds.size, 1001, "every slider position is its own speed");

  // Out of range is clamped, not extrapolated: a visitor cannot ask for 0 mps.
  assert.equal(sliderToMps(-1), MPS_MIN);
  assert.equal(sliderToMps(2), MPS_MAX);
  assert.equal(sliderToMps(Number.NaN), MPS_MIN);
  assert.equal(msPerMove(0), 1000 / MPS_MIN, "a zero speed is the slow end, not infinity");
  assert.equal(msPerMove(1e9), 1000 / MPS_MAX, "and a huge one is the fast end");
});

test("the slider maps logarithmically and round-trips", () => {
  assert.ok(Math.abs(sliderToMps(0) - MPS_MIN) < 1e-12, "the left end");
  assert.ok(Math.abs(sliderToMps(1) - MPS_MAX) < 1e-12, "the right end");

  let last = -Infinity;
  for (let index = 0; index <= 500; index += 1) {
    const mps = sliderToMps(index / 500);
    assert.ok(mps > last, "the mapping is strictly increasing");
    last = mps;
    assert.ok(Math.abs(mpsToSlider(mps) - index / 500) < 1e-9, "position round-trips");
  }

  // Log scale: equal slider steps are equal *ratios*, which is what makes the slow
  // end usable — the midpoint is the geometric mean of the ends.
  assert.ok(Math.abs(sliderToMps(0.5) - Math.sqrt(MPS_MIN * MPS_MAX)) < 1e-9);
  assert.ok(
    Math.abs(sliderToMps(mpsToSlider(MPS_DEFAULT)) - MPS_DEFAULT) < 1e-9,
    "the default has a slider position too, and it maps back to itself",
  );
});

test("a phase plan never spends more time than the Move has", () => {
  const speeds = [0.25, 0.5, 1, 1.43, 2, 3.7, 4, 8, 12, 16, 24, 32, 40];
  for (const frameMs of [FRAME_MS, 1000 / 120, 1000 / 30]) {
    for (const mps of speeds) {
      const stepMs = msPerMove(mps);
      for (let cleared = 0; cleared <= 4; cleared += 1) {
        const plan = phases(stepMs, cleared, frameMs);
        const label = `${mps} mps, ${cleared} rows, ${frameMs.toFixed(2)} ms frames`;
        assert.equal(plan.stepMs, stepMs, `${label}: the step is the step`);
        for (const key of ["dropMs", "clearMs", "contractMs", "fallMs", "holdMs"]) {
          assert.ok(Number.isFinite(plan[key]), `${label}: ${key} is a number`);
          assert.ok(plan[key] >= 0, `${label}: ${key} is not negative`);
        }
        const spent = plan.dropMs + plan.clearMs + plan.holdMs;
        assert.ok(spent <= stepMs + 1e-9, `${label}: the budget is not overspent (${spent} > ${stepMs})`);
        assert.ok(stepMs - spent < 1e-6, `${label}: and the time is accounted for, not discarded`);
        if (cleared === 0) {
          assert.equal(plan.clearMs, 0, `${label}: no clear, no clear time`);
          assert.equal(plan.contractMs, 0, `${label}: and no band to contract`);
          assert.equal(plan.fallMs, 0, `${label}: and no segment to fall`);
        } else if (plan.mode !== "skip") {
          assert.ok(plan.clearMs > 0, `${label}: a clear gets time`);
          assert.ok(
            Math.abs(plan.contractMs + plan.fallMs - plan.clearMs) < 1e-9,
            `${label}: the clear is a contract then a fall`,
          );
          assert.ok(plan.contractMs > 0 && plan.fallMs > 0, `${label}: both halves happen`);
        }
      }
    }
  }
});

test("a heavier clear gets more time, but not proportionally more", () => {
  for (const mps of [1, 1.43, 2, 4, 8, 16]) {
    const one = phases(msPerMove(mps), 1, FRAME_MS);
    const four = phases(msPerMove(mps), 4, FRAME_MS);
    assert.ok(four.clearMs > one.clearMs, `${mps} mps: four rows are visibly heavier than one`);
    assert.ok(
      four.clearMs <= one.clearMs * 1.6,
      `${mps} mps: four rows are not four times slower (${four.clearMs} vs ${one.clearMs})`,
    );
  }
  // Across the whole range the phase is monotone in the number of rows: a four-row
  // clear is never cheaper than a one-row clear at any speed.
  for (let index = 0; index <= 200; index += 1) {
    const mps = sliderToMps(index / 200);
    const times = [0, 1, 2, 3, 4].map((rows) => phases(msPerMove(mps), rows, FRAME_MS).clearMs);
    for (let rows = 1; rows < times.length; rows += 1) {
      assert.ok(times[rows] >= times[rows - 1], `${mps.toFixed(2)} mps: clear time never decreases`);
    }
  }
});

test("faster than a frame, the plan says so instead of pretending to animate", () => {
  for (const frameMs of [FRAME_MS, 1000 / 120, 1000 / 30]) {
    const shortStep = frameMs * 0.5;
    const skipped = phases(shortStep, 2, frameMs);
    assert.equal(skipped.mode, "skip", "a sub-frame step skips the animation");
    assert.equal(skipped.dropMs + skipped.clearMs, 0, "and draws no intermediate state");
    assert.ok(Math.abs(skipped.holdMs - skipped.stepMs) < 1e-9, "the whole step is the hold");

    const normal = phases(msPerMove(1.43), 2, frameMs);
    assert.equal(normal.mode, "full", "the default pace animates in full");

    const tight = phases(frameMs * 2, 2, frameMs);
    assert.equal(tight.mode, "compact", "two frames a step is compact: no hold, flat shares");
    assert.ok(Math.abs(tight.holdMs) < 1e-9, "compact spends the whole step on drop and clear");
    assert.ok(tight.dropMs > 0 && tight.clearMs > 0, "and still has both phases");
  }
});

test("the drop is a real share of the step, at every speed the page offers", () => {
  for (let index = 0; index <= 100; index += 1) {
    const mps = sliderToMps(index / 100);
    const plan = phases(msPerMove(mps), 1, FRAME_MS);
    if (plan.mode === "skip") continue;
    assert.ok(
      plan.dropMs >= plan.stepMs * 0.4,
      `${mps.toFixed(2)} mps: the drop is most of the step, so it reads as a drop`,
    );
    assert.ok(plan.dropMs <= plan.stepMs, `${mps.toFixed(2)} mps: and the step contains it`);
    if (plan.mode === "full") {
      assert.ok(
        plan.holdMs >= plan.stepMs * 0.05 - 1e-9,
        `${mps.toFixed(2)} mps: a beat of rest is what makes a step read as a step`,
      );
    }
  }
});

test("phases are a pure function of (step, rows, frame)", () => {
  const a = phases(699.3, 2, FRAME_MS);
  const b = phases(699.3, 2, FRAME_MS);
  assert.deepEqual(a, b, "same inputs, same plan");
  const slower = phases(msPerMove(0.5), 2, FRAME_MS);
  assert.ok(slower.stepMs > a.stepMs, "half the speed is twice the step");
  assert.ok(slower.dropMs > a.dropMs, "and the drop is longer by the same ratio");
  assert.ok(
    Math.abs(slower.dropMs / slower.stepMs - a.dropMs / a.stepMs) < 1e-9,
    "the *shape* of the Move does not change with speed",
  );
});
