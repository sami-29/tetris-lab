// Seam: the two numbers the page hangs on come from the committed width curve.
//
// Ticket 06 (§9.5): `DEFAULT_BEAM` and the Width cap live in
// `data/measurements/width-curve.json`, written by `bench width-curve`, and the
// page reads them at build time. This file is the drift guard: it imports the
// page's own module — `src/config/width.ts`, the only module allowed to name the
// numbers — and compares what JavaScript actually gets against a fresh parse of
// the artifact. A hand-edited artifact, a rewritten module or a hard-coded width
// in `main.ts` fails here before it can ship.
//
//   node scripts/build-wasm.mjs && node --test "test/**/*.test.mjs"
//
// The artifact itself is parsed with `JSON.parse`, so this file is also the one
// place that asserts the committed file is valid JSON with the shape §9.5 needs.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { DEFAULT_BEAM, WIDTH_CAP } from "../src/config/width.ts";

const HERE = new URL(".", import.meta.url);
const ARTIFACT_PATH = fileURLToPath(new URL("../../data/measurements/width-curve.json", HERE));
const WEB = fileURLToPath(new URL("../", HERE));
const ROOT = fileURLToPath(new URL("../../", HERE));

const artifact = JSON.parse(readFileSync(ARTIFACT_PATH, "utf8"));
const artifactText = readFileSync(ARTIFACT_PATH, "utf8");

/** §9.3's two budgets in ms, read from the artifact, keyed by the bed they bound. */
const BUDGETS = {
  "goal8-p40-cap7": artifact.statistic.budgets_ms.default_ms,
  "goal12-p100-cap7": artifact.statistic.budgets_ms.harder_ms,
};

test("the page imports the numbers the artifact records", () => {
  assert.equal(DEFAULT_BEAM, artifact.default_beam);
  assert.equal(WIDTH_CAP, artifact.width_cap);
  assert.equal(typeof DEFAULT_BEAM, "number");
  assert.equal(typeof WIDTH_CAP, "number");
  assert.ok(Number.isInteger(DEFAULT_BEAM) && Number.isInteger(WIDTH_CAP));
});

test("the default is a measured doubling point and the cap is the slider maximum", () => {
  const widths = artifact.protocol.widths;
  assert.ok(Array.isArray(widths) && widths.length >= 2, "protocol.widths");
  assert.ok(widths.includes(DEFAULT_BEAM), `default ${DEFAULT_BEAM} is a measured width`);
  assert.ok(widths.includes(WIDTH_CAP), `cap ${WIDTH_CAP} is a measured width`);
  assert.ok(DEFAULT_BEAM >= 1);
  assert.ok(DEFAULT_BEAM <= WIDTH_CAP, "the field may not open past the slider's end");
});

test("the budgets are 200 ms and 1 s, and the 50 ms figure is retired", () => {
  assert.equal(artifact.statistic.budgets_ms.default_ms, 200);
  assert.equal(artifact.statistic.budgets_ms.harder_ms, 1000);
  assert.equal(artifact.statistic.budgets_ms.default_bed, "goal8-p40-cap7");
  assert.equal(artifact.statistic.budgets_ms.harder_bed, "goal12-p100-cap7");
  assert.deepEqual(BUDGETS, { "goal8-p40-cap7": 200, "goal12-p100-cap7": 1000 });
  // §9.3: "13's default-width single Solve under 50 ms is retired as a rule".
  assert.ok(!/50\s*ms/.test(artifactText), "no 50 ms budget in the artifact");
  assert.ok(!/fifty/i.test(artifactText), "no prose 50 ms budget in the artifact");
});

test("the tie rule is the pre-registered one: exact McNemar plus a 1.0 pp floor", () => {
  assert.equal(artifact.statistic.alpha, 0.01);
  assert.equal(artifact.statistic.max_shortfall_pp, 1);
  assert.equal(artifact.statistic.max_shortfall_tenths_pp, 10);
  assert.match(artifact.statistic.name, /mcnemar/i);
});

test("the artifact is tied to the committed Dataset", () => {
  for (const [name, budget] of Object.entries(BUDGETS)) {
    const manifest = JSON.parse(readFileSync(`${ROOT}/data/dataset/${name}/manifest.json`, "utf8"));
    assert.equal(artifact.dataset[name].fingerprint, manifest.fingerprint, `${name} fingerprint`);
    assert.equal(artifact.dataset[name].generator_version, manifest.generator_version);
    assert.ok(budget > 0);
  }
  assert.equal(artifact.dataset["goal16-p160-cap7"], undefined, "the ranking bed is not committed data");
});

test("the artifact records the bed that ranked the widths and why", () => {
  const saturation = artifact.saturation;
  assert.equal(saturation.checked_at_width, WIDTH_CAP, "§9.1's pilot runs at the cap");
  for (const name of Object.keys(BUDGETS)) {
    const counted = saturation.beds[name];
    assert.ok(counted, `${name} in the pilot`);
    assert.ok(counted.wins <= counted.games);
  }
  assert.equal(typeof saturation.quality_bed, "string");
  assert.ok(saturation.fallback_reason.length > 10, "a sentence, not a token");
  assert.ok(
    ["resolved", "no-resolution"].includes(artifact.quality_axis),
    `quality_axis ${artifact.quality_axis}`,
  );
  if (artifact.quality_axis === "no-resolution") {
    assert.equal(artifact.default_beam_basis, "cost", "do not dress a cost decision up as quality");
  }
});

test("the cap is the largest measured width whose p90 fits both budgets", () => {
  const passing = [];
  for (const point of artifact.points) {
    const fits = point.cost.every((bed) => bed.ms_p90 <= BUDGETS[bed.bed]);
    if (fits) passing.push(point.width);
    assert.ok(bed_is_complete(point), `width ${point.width} records both shipped beds`);
  }
  const largest = Math.max(...passing);
  assert.equal(WIDTH_CAP, largest);
  // And the word "largest" is only true if the curve has evidence of a width above
  // it that failed: a measured failing point, or a probed one. `schedule` is the
  // word for "the pre-registered list ran out with every point passing", and the
  // tooltip built on it says the curve was still climbing — so it is only reachable
  // when the whole list was measured.
  const failing = artifact.points.filter(
    (point) => !point.cost.every((bed) => bed.ms_p90 <= BUDGETS[bed.bed]),
  );
  const probe = artifact.overshoot;
  if (failing.length > 0) {
    assert.equal(artifact.cap_bound, "budget");
    assert.ok(failing[0].width > WIDTH_CAP);
  } else if (probe) {
    assert.equal(artifact.cap_bound, "budget");
    assert.ok(probe.width > WIDTH_CAP, "the probe is above the cap");
  } else {
    assert.equal(artifact.cap_bound, "schedule");
    assert.equal(artifact.complete, true, "only a fully measured curve can name the schedule");
  }
});

test("the artifact says which pre-registered widths it did not measure, and why", () => {
  // §9.5's doubling points, which the curve may stop inside but never re-pick.
  const PRE_REGISTERED = [25, 50, 100, 200, 400, 800, 1600, 3200, 6400];
  const measured = artifact.points.map((point) => point.width);
  const unmeasured = PRE_REGISTERED.filter((width) => !measured.includes(width));
  assert.deepEqual(
    artifact.missing_widths,
    unmeasured,
    "missing_widths is the pre-registered list minus the measured points",
  );
  assert.equal(artifact.complete, unmeasured.length === 0, "complete means every point was measured");
  assert.equal(
    artifact.incomplete_reason.trim() === "",
    artifact.complete,
    "a partial curve explains itself and a complete one has nothing to explain",
  );
  // The word the tooltip hangs on cannot be the schedule unless the list ran out.
  if (artifact.cap_bound === "schedule") {
    assert.equal(artifact.complete, true, "a truncated curve was not stopped by the schedule");
  }
});

test("a probed width is evidence about the stop rule and never a point", () => {
  const probe = artifact.overshoot;
  assert.ok(probe, "the committed curve stops on a budget, so the probe is on the record");
  assert.equal(probe.probe, true, "the probe says what it is");
  assert.equal(probe.bed, artifact.statistic.budgets_ms.default_bed, "§9.5's cap criterion");
  assert.ok(probe.ms_p90 > BUDGETS[probe.bed], `probe p90 ${probe.ms_p90} fails ${BUDGETS[probe.bed]}`);
  assert.ok(probe.samples > 0, "a p90 without a sample size is not a measurement");
  assert.equal(typeof probe.substrate, "string");
  // Not a point: absent from the series, and named in the widths the file says it
  // did not measure.
  assert.ok(!artifact.points.some((point) => point.width === probe.width));
  assert.ok(artifact.missing_widths.includes(probe.width));
  assert.ok(probe.note.length > 0, "the operator's sentence travels with the number");
});

function bed_is_complete(point) {
  const names = point.cost.map((bed) => bed.bed).sort();
  return (
    names.length === 2 &&
    names[0] === "goal12-p100-cap7" &&
    names[1] === "goal8-p40-cap7" &&
    point.cost.every((bed) => typeof bed.ms_p90 === "number" && bed.ms_p90 > 0)
  );
}

test("every measured point carries its trials, so the timing spread is visible", () => {
  for (const point of artifact.points) {
    for (const bed of point.cost) {
      assert.ok(Array.isArray(bed.trials_ms_p90), `width ${point.width} ${bed.bed} trials`);
      assert.equal(bed.trials_ms_p90.length, artifact.protocol.cost_trials);
      assert.ok(bed.trials_ms_p90.every((ms) => typeof ms === "number" && ms > 0));
      assert.ok(bed.ms_p90 > 0 && bed.ms_p50 > 0);
    }
  }
});

test("the page names the numbers in one module and nowhere else", () => {
  const widthModule = readFileSync(`${WEB}/src/config/width.ts`, "utf8");
  assert.match(
    widthModule,
    /data\/measurements\/width-curve\.json/,
    "width.ts imports the artifact",
  );

  // Ticket 13 moved the default *game* out of `main.ts` into `src/core/game.ts`,
  // where it takes the two numbers as a `Bounds` argument. So the guard follows the
  // number: the page builds one pair from the artifact, the URL default and the
  // Playground's slider both read that pair, and neither types a width.
  const main = readFileSync(`${WEB}/src/main.ts`, "utf8");
  assert.match(main, /defaultBeam:\s*DEFAULT_BEAM/, "the URL default is DEFAULT_BEAM");
  assert.match(main, /widthCap:\s*WIDTH_CAP/, "the slider's end is WIDTH_CAP");
  assert.match(main, /from "\.\/config\/width\.ts"/, "the page imports the module");

  const gameState = readFileSync(`${WEB}/src/core/game.ts`, "utf8");
  assert.match(gameState, /bounds\.defaultBeam/, "the default game reads the bounds");
  assert.doesNotMatch(gameState, /beamWidth:\s*\d+/, "no hard-coded beam width in the URL state");

  const playground = readFileSync(`${WEB}/src/ui/playground.ts`, "utf8");
  assert.match(playground, /boundOf\(field, this\.bounds\)/, "the slider's bounds are computed");
  assert.doesNotMatch(playground, /(defaultBeam|widthCap)\s*[=:]\s*\d/, "no width typed into the controls");
});
