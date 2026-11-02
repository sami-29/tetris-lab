// Seam 1: the game in the URL — parse, clamp, serialise. No DOM, no wasm, no canvas.
//
// §2.2 is the whole contract and it is three sentences long:
//
//   * the query string carries the settable game as lowercase `seed`, `goal`,
//     `pieces`, `cap`, `beam`;
//   * an absent or unparseable value falls back to the default rather than
//     erroring;
//   * every accepted value is clamped to the generator's domain (§5.8) before it
//     reaches wasm, and `beam` is additionally clamped to `1..WIDTH_CAP`.
//
// The numbers the page hangs on are passed in (`BOUNDS`), not imported: this file
// is pure `node --test` and the width artifact is `width-curve.test.mjs`'s seam.
// A literal `{ defaultBeam: 25, widthCap: 800 }` here is deliberate — it is the
// same pair the artifact records today, and if ticket 06 is ever re-measured the
// *contract* this file pins does not move.
//
// What a test here buys: a link written by one build, opened by another, names a
// game the engine accepts. §5.8: "a URL stored above a re-measured cap would start
// returning status 1 instead of a solve" — clamping once, at this edge, is what
// makes an old link degrade to a game instead of to an error.

import { test } from "node:test";
import assert from "node:assert/strict";

import {
  LIMITS,
  URL_KEYS,
  applyField,
  clampGame,
  defaultGame,
  describeGame,
  parseGame,
  parseInteger,
  sameGame,
  serialiseGame,
} from "../src/core/game.ts";

const BOUNDS = { defaultBeam: 25, widthCap: 800 };

/** §2.2's default game: seed 0, goal 8, 40 pieces, cap 7, the default width. */
const DEFAULTS = { seed: 0, goal: 8, pieces: 40, heightCap: 7, beamWidth: 25 };

// ── the defaults ────────────────────────────────────────────────────────────

test("the default game is §2.2's, with the width taken from the bounds", () => {
  assert.deepEqual(defaultGame(BOUNDS), DEFAULTS);
  // The width is never a literal: it is whatever the artifact names today.
  assert.equal(defaultGame({ defaultBeam: 33, widthCap: 400 }).beamWidth, 33);
  assert.equal(defaultGame({ defaultBeam: 400, widthCap: 400 }).beamWidth, 400);
});

test("the five keys are lowercase and in one fixed order", () => {
  assert.deepEqual([...URL_KEYS], ["seed", "goal", "pieces", "cap", "beam"]);
  for (const key of URL_KEYS) assert.equal(key, key.toLowerCase());
});

// ── the Domain (§5.8, plus the two bounds the contract does not carry) ──────

test("the Domain is §5.8's, and the Width cap narrows beam alone", () => {
  assert.deepEqual(LIMITS, {
    seed: { min: 0, max: 4294967295 },
    goal: { min: 1, max: 255 },
    pieces: { min: 1, max: 200 },
    cap: { min: 2, max: 19 },
    beam: { min: 1, max: 16384 },
  });
  // `beam` is the one field the measured cap narrows; a wider cap is not a wider
  // Domain, and a narrower cap never widens it (§2.2, §5.8).
  assert.equal(clampGame({ beamWidth: 16384 }, { defaultBeam: 25, widthCap: 800 }).beamWidth, 800);
  assert.equal(clampGame({ beamWidth: 16384 }, { defaultBeam: 25, widthCap: 16384 }).beamWidth, 16384);
  assert.equal(clampGame({ beamWidth: 400 }, { defaultBeam: 25, widthCap: 800 }).beamWidth, 400);
});

// ── "unparseable" is a whole decimal integer, or nothing ────────────────────

test("a value is a whole decimal integer or it is unparseable", () => {
  // The rule: trim, then /^[+-]?[0-9]+$/ — nothing else is a number.
  // `Number()` alone would take `0x10`, `1e3` and `7.5`; `parseInt()` alone would
  // take `40abc` and truncate `7.5`. Both are ways for a URL to name a game the
  // visitor did not type.
  assert.equal(parseInteger(null), null, "absent");
  assert.equal(parseInteger(""), null, "empty is not zero");
  assert.equal(parseInteger("   "), null, "whitespace is not zero");
  assert.equal(parseInteger("40"), 40);
  assert.equal(parseInteger(" 40 "), 40, "trimmed");
  assert.equal(parseInteger("\u00a040"), 40, "a no-break space trims too");
  assert.equal(parseInteger("+40"), 40);
  assert.equal(parseInteger("-40"), -40, "parseable, out of domain: the clamp's job");
  assert.equal(parseInteger("040"), 40);
  assert.equal(parseInteger("4294967295"), 4294967295);
  for (const notANumber of ["0x10", "1e3", "7.5", "7.0", "NaN", "Infinity", "40abc", "４０", "1,000"]) {
    assert.equal(parseInteger(notANumber), null, `${JSON.stringify(notANumber)} is not an integer`);
  }
});

// ── the clamp table ────────────────────────────────────────────────────────

const CLAMP_TABLE = [
  // parseable and inside: kept
  ["?cap=19", { ...DEFAULTS, heightCap: 19 }, "the top of the Domain"],
  ["?cap=2", { ...DEFAULTS, heightCap: 2 }, "the bottom of the Domain"],
  ["?pieces=1", { ...DEFAULTS, pieces: 1 }, "a one-piece game"],
  ["?pieces=200", { ...DEFAULTS, pieces: 200 }, "the Sequence's full length"],
  ["?seed=4294967295", { ...DEFAULTS, seed: 4294967295 }, "the whole u32"],
  ["?goal=255", { ...DEFAULTS, goal: 255 }, "the 8-bit Lines field's ceiling"],
  ["?beam=800", { ...DEFAULTS, beamWidth: 800 }, "the measured cap"],
  // parseable and outside: clamped, never an error (§2.2, §5.8)
  ["?cap=20", { ...DEFAULTS, heightCap: 19 }, "above the Domain the generator refuses"],
  ["?cap=1", { ...DEFAULTS, heightCap: 2 }, "below 2 no piece can land"],
  ["?cap=0", { ...DEFAULTS, heightCap: 2 }, "zero is a value, not an absence"],
  ["?cap=-3", { ...DEFAULTS, heightCap: 2 }, "a negative would wrap in a u32"],
  ["?cap=4294967295", { ...DEFAULTS, heightCap: 19 }, "an absurd cap clamps to the top"],
  ["?pieces=0", { ...DEFAULTS, pieces: 1 }, "a game needs one piece"],
  ["?pieces=201", { ...DEFAULTS, pieces: 200 }, "one past the Domain"],
  ["?pieces=1000", { ...DEFAULTS, pieces: 200 }, "one far past it"],
  ["?seed=-1", { ...DEFAULTS, seed: 0 }, "seeds start at zero"],
  ["?seed=99999999999", { ...DEFAULTS, seed: 4294967295 }, "clamped to the end of the u32"],
  ["?goal=0", { ...DEFAULTS, goal: 1 }, "the packed result cannot report a win at 0"],
  ["?goal=256", { ...DEFAULTS, goal: 255 }, "eight bits of Lines, so 255 is the ceiling"],
  ["?goal=9999", { ...DEFAULTS, goal: 255 }, "far above it"],
  ["?beam=0", { ...DEFAULTS, beamWidth: 1 }, "a beam of nothing is one"],
  ["?beam=-1", { ...DEFAULTS, beamWidth: 1 }, "negative width clamps up"],
  ["?beam=16384", { ...DEFAULTS, beamWidth: 800 }, "§5.8 accepts it; the UI does not offer it"],
  ["?beam=20000", { ...DEFAULTS, beamWidth: 800 }, "above even the ABI's Domain"],
  // unparseable: the default, not a clamp and not an error
  ["?cap=abc", DEFAULTS, "not a number"],
  ["?cap=", DEFAULTS, "empty"],
  ["?cap=0x7", DEFAULTS, "hex is not a decimal integer"],
  ["?cap=7.5", DEFAULTS, "a float is not silently truncated"],
  ["?cap=7.0", DEFAULTS, "nor is a whole float"],
  ["?cap=1e1", DEFAULTS, "no exponent"],
  ["?Cap=12", DEFAULTS, "keys are case-sensitive: `Cap` is absent, not different"],
  ["?beam=1e400", DEFAULTS, "Infinity is not a clamp"],
  ["?seed=", DEFAULTS, "an empty seed is the default seed"],
  // trimming is load-bearing: `+` in a query string decodes to a space
  ["?cap=+12", { ...DEFAULTS, heightCap: 12 }, "`+` decodes to a space, so trim"],
  ["?cap=%2B12", { ...DEFAULTS, heightCap: 12 }, "a literal `+` sign is a sign"],
  ["?cap=%2012%20", { ...DEFAULTS, heightCap: 12 }, "%20 around the value"],
  // a repeated key: the platform's own reading, first wins (§1.2)
  ["?cap=12&cap=19", { ...DEFAULTS, heightCap: 12 }, "first wins"],
  ["?seed=5&seed=9", { ...DEFAULTS, seed: 5 }, "first wins again"],
  // unknown keys are inert
  ["?utm_source=x&cap=12", { ...DEFAULTS, heightCap: 12 }, "a tracking parameter is invisible"],
  ["?CAP=12&cap=12", { ...DEFAULTS, heightCap: 12 }, "only the lowercase key counts"],
];

test("every URL value is clamped to the Domain, and never errors", () => {
  for (const [search, expected, why] of CLAMP_TABLE) {
    assert.deepEqual(parseGame(search, BOUNDS), expected, `${search} — ${why}`);
  }
});

test("no query string is §2.2's default game", () => {
  assert.deepEqual(parseGame("", BOUNDS), DEFAULTS);
  assert.deepEqual(parseGame("?", BOUNDS), DEFAULTS);
});

test("every link this page writes is a game the Domain accepts", () => {
  // The mechanical form of "the UI never offers a value the contract refuses to
  // name": whatever came in, what comes out is five integers inside the bounds.
  const hostile = CLAMP_TABLE.map(([search]) => search).concat([
    "?seed=1&goal=1&pieces=1&cap=1&beam=1",
    "?seed=4294967296&goal=300&pieces=300&cap=300&beam=30000",
    "?seed=-0&goal=-0&pieces=-0&cap=-0&beam=-0",
  ]);
  for (const search of hostile) {
    const game = parseGame(search, BOUNDS);
    for (const [field, limit] of Object.entries(LIMITS)) {
      const key = field === "cap" ? "heightCap" : field === "beam" ? "beamWidth" : field;
      const value = game[key];
      assert.ok(Number.isInteger(value), `${search}: ${field} is an integer`);
      assert.ok(value >= limit.min, `${search}: ${field} ${value} >= ${limit.min}`);
      const max = field === "beam" ? Math.min(limit.max, BOUNDS.widthCap) : limit.max;
      assert.ok(value <= max, `${search}: ${field} ${value} <= ${max}`);
    }
  }
});

// ── serialising: the link a visitor copies ─────────────────────────────────

test("a link leaves out the keys that are already the default", () => {
  // So "a link to the Lab" and "a link to a game" stay distinguishable, and a
  // hand-edited URL is normalised the moment the page writes.
  assert.equal(serialiseGame(DEFAULTS, BOUNDS), "");
  assert.equal(serialiseGame({ ...DEFAULTS, heightCap: 12 }, BOUNDS), "?cap=12");
  assert.equal(
    serialiseGame({ seed: 7, goal: 12, pieces: 100, heightCap: 19, beamWidth: 800 }, BOUNDS),
    "?seed=7&goal=12&pieces=100&cap=19&beam=800",
    "the key order is fixed, so two links are diffable",
  );
});

test("reloading a copied link reproduces the same game", () => {
  const games = [
    DEFAULTS,
    { seed: 4294967295, goal: 1, pieces: 1, heightCap: 2, beamWidth: 1 },
    { seed: 4294967295, goal: 255, pieces: 200, heightCap: 19, beamWidth: 800 },
    { seed: 42, goal: 12, pieces: 100, heightCap: 7, beamWidth: 400 },
    { seed: 1, goal: 8, pieces: 40, heightCap: 19, beamWidth: 25 },
  ];
  for (const game of games) {
    const search = serialiseGame(game, BOUNDS);
    assert.deepEqual(parseGame(search, BOUNDS), game, `round trip of ${search || "(empty)"}`);
  }
  // And over a sample, so the corners are not the only evidence.
  for (let seed = 0; seed < 64; seed += 7) {
    const game = {
      seed: (seed * 2654435761) >>> 0,
      goal: 1 + (seed % 255),
      pieces: 1 + (seed % 200),
      heightCap: 2 + (seed % 18),
      beamWidth: 1 + (seed % BOUNDS.widthCap),
    };
    assert.deepEqual(parseGame(serialiseGame(game, BOUNDS), BOUNDS), game, `seed ${game.seed}`);
  }
});

test("Evaluator Weights cannot appear in the URL", () => {
  // §2.2: "Evaluator weights are **not** in the URL (they are the expert's
  // transient state)". Two properties, and the first is structural: the serialiser
  // is given a five-field game and iterates the five keys, so there is no branch
  // that could reach a sixth.
  for (const game of [DEFAULTS, { seed: 3, goal: 1, pieces: 200, heightCap: 2, beamWidth: 1 }]) {
    const emitted = [...new URLSearchParams(serialiseGame(game, BOUNDS)).keys()];
    for (const key of emitted) {
      assert.ok(URL_KEYS.includes(key), `${key} is one of the five game keys`);
    }
    assert.ok(!/[wW]eight/.test(serialiseGame(game, BOUNDS)));
  }
  // And a hand-edited or stale link cannot smuggle them in: the parser reads the
  // five names and nothing else, so two links with the same five keys mean the
  // same game whatever else they carry.
  assert.deepEqual(
    parseGame("?weights=-1,1,-1,-1,-4,-1&seed=3&toggles=shipped&cap=12", BOUNDS),
    { ...DEFAULTS, seed: 3, heightCap: 12 },
  );
});

// ── what the page says about a game ────────────────────────────────────────

test("every value of the game is named in words", () => {
  const words = describeGame(DEFAULTS);
  for (const phrase of ["seed 0", "goal 8 lines", "40 pieces", "starting height 7", "beam 25"]) {
    assert.ok(words.includes(phrase), `${JSON.stringify(words)} names ${phrase}`);
  }
});

test("sameGame compares the five values, nothing else", () => {
  assert.ok(sameGame(DEFAULTS, { ...DEFAULTS }));
  for (const field of ["seed", "goal", "pieces", "heightCap", "beamWidth"]) {
    assert.ok(!sameGame(DEFAULTS, { ...DEFAULTS, [field]: DEFAULTS[field] + 1 }), field);
  }
});

// ── one control changes one field ─────────────────────────────────────────

test("a control changes its own field and leaves the other four alone", () => {
  const start = { seed: 3, goal: 12, pieces: 100, heightCap: 7, beamWidth: 25 };
  const cases = [
    ["seed", "9", { seed: 9 }],
    ["goal", "20", { goal: 20 }],
    ["pieces", "120", { pieces: 120 }],
    ["cap", "19", { heightCap: 19 }],
    ["beam", "400", { beamWidth: 400 }],
  ];
  for (const [field, raw, expected] of cases) {
    assert.deepEqual(applyField(start, field, raw, BOUNDS), { ...start, ...expected }, field);
  }
  // A field that will not parse moves itself to its own default and nothing else.
  assert.deepEqual(applyField(start, "goal", "", BOUNDS), { ...start, goal: 8 });
  assert.deepEqual(applyField(start, "cap", "tall", BOUNDS), { ...start, heightCap: 7 });
  // And an out-of-domain value clamps rather than erroring.
  assert.deepEqual(applyField(start, "pieces", "5000", BOUNDS), { ...start, pieces: 200 });
  // Changing the starting height cannot reach the Sequence's inputs: it changes
  // `heightCap` and not `seed` or `pieces` (§5.7's invariant, at the URL's level).
  const raised = applyField(start, "cap", "19", BOUNDS);
  assert.equal(raised.seed, start.seed);
  assert.equal(raised.pieces, start.pieces);
});
