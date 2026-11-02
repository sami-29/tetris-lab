// Seam 1: the one line that explains why the width field opens where it does.
//
// §2.2: "The toolbar's width field opens at `DEFAULT_BEAM`; the slider runs to the
// cap with a labelled tick at the default, and one tooltip line explains why they
// differ ('the narrowest width statistically as good as the widest we can afford').
// If the default *is* the cap, the tick merges into the slider's end and the
// tooltip says the timeout is what stopped it."
//
// That quoted sentence is the *pre-registered* case. What actually happened is in
// the artifact, and it says something else: the quality axis found no resolution
// (`quality_axis: "no-resolution"`), so `DEFAULT_BEAM` was named **on cost**
// (`default_beam_basis: "cost"`), and the sweep ended where the pre-registered
// doubling list ended (`cap_bound: "schedule"`), not at a time limit. A tooltip
// that claimed a statistical tie would be a lie in the UI, so the sentence is
// written from the fields — the page reads them and composes, rather than typing a
// number or a claim it cannot check.
//
// The shipped sentence is asserted here against the fields it is derived from, so
// a re-measured artifact changes the copy and fails nothing; a *hand-written* claim
// that stops being true fails immediately.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { CAP_BOUND, DEFAULT_BEAM, DEFAULT_BEAM_BASIS, QUALITY_AXIS, WIDTH_CAP } from "../src/config/width.ts";
import { SHIPPED_WIDTH_STORY, widthTooltip } from "../src/ui/width-copy.ts";

const ARTIFACT = fileURLToPath(new URL("../../data/measurements/width-curve.json", import.meta.url));
const artifact = JSON.parse(readFileSync(ARTIFACT, "utf8"));

test("the page reads what named the default and what ended the cap", () => {
  assert.equal(DEFAULT_BEAM_BASIS, artifact.default_beam_basis);
  assert.equal(CAP_BOUND, artifact.cap_bound);
  assert.equal(QUALITY_AXIS, artifact.quality_axis);
  assert.ok(["cost", "tie"].includes(DEFAULT_BEAM_BASIS));
  assert.ok(["budget", "schedule"].includes(CAP_BOUND));
  assert.ok(["no-resolution", "resolved"].includes(QUALITY_AXIS));
});

test("the shipped line is one line, and it names both numbers", () => {
  const line = widthTooltip();
  assert.equal(typeof line, "string");
  assert.equal(line.split("\n").length, 1, "one tooltip line");
  assert.ok(line.includes(String(DEFAULT_BEAM)), `names the default ${DEFAULT_BEAM}`);
  assert.ok(line.includes(String(WIDTH_CAP)), `names the cap ${WIDTH_CAP}`);
  assert.match(line, /the default|Default/);
});

test("the shipped line does not claim a quality result the artifact does not have", () => {
  const line = widthTooltip();
  // The measured facts: no width beat another on the ranking bed, so the narrowest
  // point was chosen on cost. §2.2's pre-registered wording ("statistically as
  // good as") would be a claim the artifact contradicts.
  assert.equal(SHIPPED_WIDTH_STORY.qualityAxis, "no-resolution");
  assert.equal(SHIPPED_WIDTH_STORY.basis, "cost");
  assert.doesNotMatch(line, /optimum|optimal/i, "do not claim a quality optimum");
  assert.doesNotMatch(line, /statistically as good/i, "the tie did not happen");
  assert.match(line, /cost/i, "it says the choice was made on cost");
  assert.match(line, /could not be separated|no resolution|tied/i, "and why quality could not decide");
});

test("the line says what ended the sweep, because that is what the field records", () => {
  const shipped = widthTooltip();
  assert.equal(SHIPPED_WIDTH_STORY.capBound, "schedule");
  assert.match(shipped, /schedule/i, "cap_bound: schedule is the doubling list running out");
  assert.doesNotMatch(shipped, /time limit/i, "so it must not blame a timeout");

  const budget = widthTooltip({ ...SHIPPED_WIDTH_STORY, capBound: "budget" });
  assert.match(budget, /budget|time limit/i, "cap_bound: budget is the timeout");
  assert.doesNotMatch(budget, /schedule/i);
});

test("a re-measured default that ties the cap is described as a tie", () => {
  const tied = widthTooltip({
    ...SHIPPED_WIDTH_STORY,
    basis: "tie",
    qualityAxis: "resolved",
  });
  assert.match(tied, /as wide as|as often|tie/i, "a real tie is reported as one");
  assert.doesNotMatch(tied, /cost/i);
});

test("a default that is the cap is one sentence, not a difference", () => {
  const same = widthTooltip({
    defaultBeam: 800,
    widthCap: 800,
    basis: "cost",
    qualityAxis: "no-resolution",
    capBound: "budget",
  });
  assert.match(same, /800/);
  assert.match(same, /default is the cap/i);
  assert.doesNotMatch(same, /narrow/i, "there is no narrower width to point at");
});
