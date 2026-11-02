// Seam 1 (with one foot in the committed data): the two shipped configurations,
// as buttons.
//
// §6 commits exactly two Dataset configurations — goal 8/40/cap 7 and goal
// 12/100/cap 7 — and §9.6's Reference run quotes its figures per configuration at
// `DEFAULT_BEAM`. A preset therefore has to fill all five game values, and it has
// to fill them with the *committed* numbers, not with a copy of them that can
// drift: this file reads the manifests. That is the whole guard — a preset button
// cannot quote a configuration the committed Dataset does not have, and a
// re-measured `DEFAULT_BEAM` moves the button with the Reference rows in the same
// commit (§9.5's golden-number rule).
//
// The figures themselves are re-measured nowhere here: `playground-wasm.test.mjs`
// solves both presets once at the default width and checks the game they name is
// the game `games.txt` commits for seed 0.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { PRESETS, presetGame } from "../src/ui/presets.ts";
import { defaultGame, parseGame, serialiseGame } from "../src/core/game.ts";

const BOUNDS = { defaultBeam: 25, widthCap: 800 };
const DATASET = fileURLToPath(new URL("../../data/dataset/", import.meta.url));

test("there are two presets, and their ids are the committed directory names", () => {
  const committed = readdirSync(DATASET, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort();
  assert.deepEqual(PRESETS.map((preset) => preset.id).sort(), committed);
});

test("each preset is the committed configuration, field for field", () => {
  for (const preset of PRESETS) {
    const manifest = JSON.parse(readFileSync(`${DATASET}${preset.id}/manifest.json`, "utf8"));
    assert.equal(preset.settings.goal, manifest.goal, `${preset.id}: goal`);
    assert.equal(preset.settings.pieces, manifest.pieces, `${preset.id}: pieces`);
    assert.equal(preset.settings.heightCap, manifest.cap, `${preset.id}: cap`);
    // §2.2: the default Dataset configuration is at seed 0; §9.6: all solves at
    // DEFAULT_BEAM, so a preset that filled any other width could not reproduce
    // the quoted figures.
    assert.equal(preset.settings.seed, 0, `${preset.id}: seed`);
  }
});

test("a preset fills all five game values and one click solves it", () => {
  for (const preset of PRESETS) {
    const game = presetGame(preset, BOUNDS);
    assert.deepEqual(Object.keys(game).sort(), [
      "beamWidth",
      "goal",
      "heightCap",
      "pieces",
      "seed",
    ]);
    assert.equal(game.beamWidth, BOUNDS.defaultBeam, `${preset.id}: the widths read the artifact`);
    // The width follows the bounds, so a re-measured default moves the preset.
    assert.equal(presetGame(preset, { defaultBeam: 33, widthCap: 400 }).beamWidth, 33);
  }
});

test("a preset is URL state, not a separate mechanism", () => {
  // Clicking one runs the same clamp → replaceState → re-solve path as editing a
  // field, so a preset link is the keys that differ from the default and nothing
  // else.
  const [shipped, harder] = PRESETS;
  assert.equal(serialiseGame(presetGame(shipped, BOUNDS), BOUNDS), "");
  assert.equal(serialiseGame(presetGame(harder, BOUNDS), BOUNDS), "?goal=12&pieces=100");
  // And the link it writes parses back to the same game.
  for (const preset of PRESETS) {
    const game = presetGame(preset, BOUNDS);
    assert.deepEqual(parseGame(serialiseGame(game, BOUNDS), BOUNDS), game);
  }
});

test("the label and the tooltip name the figures, not the code", () => {
  for (const preset of PRESETS) {
    assert.ok(preset.label.includes(String(preset.settings.goal)), `${preset.id}: label goal`);
    assert.ok(preset.label.includes(String(preset.settings.pieces)), `${preset.id}: label pieces`);
    assert.ok(preset.label.length < 40, "a label, not a paragraph");

    const tooltip = preset.tooltip(BOUNDS);
    // Every number in the tooltip is interpolated: the same call with other bounds
    // produces the other bounds' numbers.
    assert.ok(tooltip.includes(String(BOUNDS.defaultBeam)), `${preset.id}: default width`);
    const other = preset.tooltip({ defaultBeam: 33, widthCap: 400 });
    assert.ok(other.includes("33"));
    assert.ok(!other.includes(` ${BOUNDS.defaultBeam} `), "not a typed-in 25");
    // It says what the button is for: reproducing a quoted figure.
    assert.match(tooltip, /reference|figure/i, `${preset.id}: says what it reproduces`);
    assert.match(tooltip, /seed 0/, "the seed is part of the configuration");
  }
});

test("the default game is the first preset", () => {
  // §2.2's default *is* the goal 8 configuration, so the site opens on a preset.
  assert.deepEqual(presetGame(PRESETS[0], BOUNDS), defaultGame(BOUNDS));
});
