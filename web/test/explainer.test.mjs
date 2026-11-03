// The Explainer, asserted where it is mechanical.
//
// Ticket 16 rewrites the drawer's text for the engine that shipped, and its
// acceptance is mostly a writing brief: the right mechanisms, the glossary's
// words, no trace of the Python app it replaces, and every quoted figure owned by
// an artifact. Three of those are testable without pretending to judge prose:
//
//   * **the facts** — `src/generated/engine-facts.ts` is generated from the
//     committed artifacts, and this file re-reads those artifacts with its own
//     parser and compares. A re-measured `DEFAULT_BEAM`, a new Dataset
//     configuration, a moved `TAIL` or a committed Reference row therefore fails
//     here before the drawer can go stale;
//   * **the vocabulary** — the drawer's rendered text is run through the same word
//     list `scripts/check-words.mjs` uses on the built page, in both tiers (the
//     hard tokens and the glossary's near-synonyms), so a banned word is a red
//     test and not a review note;
//   * **the numbers** — every literal part of the prose (the segments that are not
//     a fact) contains no digit, so a figure can only reach the page through a
//     slot whose artifact this file checks. The weight block is the shipped table,
//     eight rows, in the engine's order.
//
// What it does not do: judge whether the prose is good. `scripts/browser-check.mjs`
// opens the drawer in a real browser and reads what a visitor reads.

import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import { join, resolve } from "node:path";

import { ENGINE_FACTS } from "../src/generated/engine-facts.ts";
import { DEFAULT_BEAM, WIDTH_CAP, DEFAULT_BEAM_BASIS, QUALITY_AXIS } from "../src/config/width.ts";
import { SHIPPED_EVALUATOR, decimalOf } from "../src/config/evaluator.ts";
import {
  EXPLAINER_SECTIONS,
  EXPLAINER_LEADS,
  beamStory,
  explainerText,
  literalSegments,
  weightRows,
} from "../src/ui/explainer.ts";
import { resolveKey } from "../src/ui/keys.ts";
import { TIER_A, TIER_B, formatHits, scanFile, scanText } from "../scripts/words.mjs";

const WEB = resolve(fileURLToPath(new URL(".", import.meta.url)), "..");
const ROOT = resolve(WEB, "..");
const read = (...parts) => readFileSync(join(...parts), "utf8");

const html = read(WEB, "index.html");
const manifestNames = readdirSync(join(ROOT, "data", "dataset"), { withFileTypes: true })
  .filter((entry) => entry.isDirectory())
  .map((entry) => entry.name)
  .sort();
const manifests = manifestNames.map((name) => ({
  name,
  ...JSON.parse(read(ROOT, "data", "dataset", name, "manifest.json")),
}));

// ── the facts the drawer quotes ─────────────────────────────────────────────

test("the Tail length is the engine's constant, not a copy", () => {
  const rust = read(ROOT, "engine", "src", "search.rs");
  const match = rust.match(/pub const TAIL: usize = (\d+);/);
  assert.ok(match, "engine/src/search.rs still declares TAIL");
  assert.equal(ENGINE_FACTS.tail.pieces, Number(match[1]), "the generated fact is the engine's number");
  assert.match(ENGINE_FACTS.tail.source, /engine\/src\/search\.rs/, "and it says where it came from");
});

test("the shipped table is the one the ablation ladder named", () => {
  const ladder = JSON.parse(read(ROOT, "data", "measurements", "ablation-ladder.json"));
  assert.equal(ENGINE_FACTS.shipped.evaluator, ladder.shipped.evaluator);
  assert.equal(ENGINE_FACTS.shipped.table, ladder.shipped.table);
  assert.equal(ENGINE_FACTS.shipped.features, SHIPPED_EVALUATOR.features.length, "the page shows all of them");
  assert.equal(ENGINE_FACTS.shipped.evaluator.toUpperCase(), SHIPPED_EVALUATOR.model, "the ladder's id is the page's model");
  assert.equal(ENGINE_FACTS.shipped.table, "E1_PUBLISHED", "Dellacherie–Thiery at published weights ships");
  assert.deepEqual(
    [...ENGINE_FACTS.shipped.deletedToggles].sort(),
    ["dedup-off", "no-suicide-off"],
    "Dedup and the No-suicide rule have no off-switch left; the Tail search is the one mechanism that does",
  );
});

test("the Dataset numbers are the committed manifests'", () => {
  assert.equal(ENGINE_FACTS.dataset.configCount, manifests.length);
  assert.equal(ENGINE_FACTS.dataset.configs.length, manifests.length);
  for (const manifest of manifests) {
    const fact = ENGINE_FACTS.dataset.configs.find((config) => config.name === manifest.name);
    assert.ok(fact, `${manifest.name} is in the facts`);
    assert.equal(fact.goal, manifest.goal);
    assert.equal(fact.pieces, manifest.pieces);
    assert.equal(fact.cap, manifest.cap);
    assert.equal(fact.games, manifest.game_count);
    assert.deepEqual(fact.test, manifest.splits.test, `${manifest.name}: test split`);
    assert.deepEqual(fact.tuning, manifest.splits.tuning, `${manifest.name}: tuning split`);
    assert.equal(fact.fingerprint, manifest.fingerprint);
    assert.equal(ENGINE_FACTS.dataset.gameCount, manifest.game_count, "every configuration has the same game count");
    assert.deepEqual(ENGINE_FACTS.dataset.test, manifest.splits.test, "and the same splits");
    assert.deepEqual(ENGINE_FACTS.dataset.tuning, manifest.splits.tuning);
  }
});

test("a Reference row, when one is committed, is read; while none is, the drawer says so", () => {
  const rows = manifestNames.flatMap((name) => {
    const text = read(ROOT, "data", "dataset", name, "reference.jsonl");
    return text.split(/\r?\n/).filter((line) => line.trim()).map((line) => JSON.parse(line));
  });
  if (rows.length === 0) {
    assert.equal(ENGINE_FACTS.reference.status, "pending", "no row, no quoted machine");
    assert.equal(ENGINE_FACTS.reference.rows.length, 0);
  } else {
    assert.equal(ENGINE_FACTS.reference.status, "measured");
    assert.equal(ENGINE_FACTS.reference.rows.length, manifestNames.length, "the latest row for each configuration");
    for (const row of ENGINE_FACTS.reference.rows) {
      for (const key of ["machine", "runtime", "date", "commit"]) {
        assert.ok(row[key] && row[key].length > 0, `${row.config}: ${key} is labelled`);
      }
    }
  }
});

// ── the text ────────────────────────────────────────────────────────────────

test("the drawer has the nine sections the shipped search needs, in reading order", () => {
  assert.deepEqual(
    EXPLAINER_LEADS,
    ["The Lab", "Beam width", "Dedup", "Tail search", "No-suicide rule", "Evaluator", "Dataset", "Reference run", "Run"],
    "one section per mechanism, in the order a visitor meets them",
  );
  for (const section of EXPLAINER_SECTIONS) {
    const body = section.body.map((segment) => (typeof segment === "string" ? segment : "x")).join("").trim();
    assert.ok(body.length > 80, `${section.lead} has a paragraph, not a placeholder`);
  }
});

test("every quoted figure resolves from a fact the artifacts own", () => {
  const text = explainerText();
  assert.ok(text.includes(String(DEFAULT_BEAM)), "the default Beam width");
  assert.ok(text.includes(String(WIDTH_CAP)), "the Width cap");
  assert.ok(text.includes(String(ENGINE_FACTS.tail.pieces)), "the Tail length");
  assert.ok(text.includes(String(SHIPPED_EVALUATOR.features.length)), "the Feature count");
  assert.ok(text.includes(SHIPPED_EVALUATOR.name), "the shipped Evaluator's name");
  assert.ok(text.includes(String(ENGINE_FACTS.dataset.configCount)), "the Dataset configuration count");
  assert.ok(text.includes(ENGINE_FACTS.dataset.games), "the games per configuration");
  assert.ok(text.includes(ENGINE_FACTS.dataset.testRange), "the test Split's seeds");
  assert.ok(text.includes(ENGINE_FACTS.dataset.tuneRange), "the tuning Split's seeds");
});

test("the prose contains no digit of its own: every number is a slot", () => {
  for (const segment of literalSegments()) {
    assert.doesNotMatch(segment, /[0-9]/, `typed number in: ${JSON.stringify(segment.slice(0, 80))}`);
  }
});

test("the drawer is written in the glossary's words and free of the old stack", () => {
  const hits = scanText(explainerText(), "src/ui/explainer.ts", { tiers: [TIER_A, TIER_B] });
  assert.deepEqual(hits, [], formatHits(hits, 1));
  // The module's own text, at real line numbers, for Tier A only: the source ships
  // inside `dist/assets/*.js.map`, so a hard token in a comment is delivered to the
  // browser. Tier B is a prose rule and the module's identifiers (`config-count`, an
  // import path) are not prose.
  const source = scanFile(join(WEB, "src", "ui", "explainer.ts"), { tiers: [TIER_A] });
  assert.deepEqual(source, [], "no hard token reaches the source map");
});

test("the weight block is the shipped table, all eight, in the engine's order", () => {
  const rows = weightRows();
  assert.equal(rows.length, SHIPPED_EVALUATOR.features.length);
  assert.deepEqual(rows.map((row) => row.label), SHIPPED_EVALUATOR.features.map((feature) => feature.label));
  assert.deepEqual(
    rows.map((row) => row.value),
    SHIPPED_EVALUATOR.features.map((feature) => decimalOf(feature.weightQ16)),
    "each Weight is the integer's rendering, not a second decimal",
  );
  // The one Feature the paper's function does not use is shown at zero, not hidden.
  assert.ok(rows.some((row) => row.value === "0.00"), "a zero Weight is visible");
});

// ── the beam story, which must be selected from the artifact's fields ──────

test("what named the default is the artifact's story, whichever story it records", () => {
  const shipped = beamStory({
    defaultBeam: DEFAULT_BEAM,
    widthCap: WIDTH_CAP,
    basis: DEFAULT_BEAM_BASIS,
    qualityAxis: QUALITY_AXIS,
  }).join("");
  assert.match(shipped, /Width cap/, "the cap is always named");
  if (DEFAULT_BEAM_BASIS === "cost" && QUALITY_AXIS === "no-resolution") {
    assert.match(shipped, /named on cost/, "the artifact's quality axis had no resolution");
    assert.doesNotMatch(shipped, /narrowest width that ties/, "so the drawer may not claim a quality tie");
  } else if (DEFAULT_BEAM_BASIS === "tie") {
    assert.match(shipped, /narrowest width that ties/, "a tie is described as a tie");
  }

  // The two degenerate stories are reachable, so a re-measurement cannot land on a
  // sentence that has never been rendered.
  assert.match(
    beamStory({ defaultBeam: 800, widthCap: 800, basis: "cost", qualityAxis: "no-resolution" }).join(""),
    /default is the cap/,
  );
  assert.match(
    beamStory({ defaultBeam: 400, widthCap: 800, basis: "tie", qualityAxis: "resolved" }).join(""),
    /narrowest width that ties/,
  );
});

// ── the drawer's home, and the way in and out ──────────────────────────────

test("the drawer's markup gives the prose a home and keeps the keyboard sheet", () => {
  assert.match(html, /id="drawer"[^>]*data-open="false"/, "the drawer is still closed on load");
  assert.match(html, /id="explainer-body"/, "the prose mounts here");
  assert.match(html, /id="sheet"[^>]*data-open="false"/, "the keyboard sheet is still a section of the drawer");
  const body = html.indexOf('id="explainer-body"');
  const sheet = html.indexOf('id="sheet"');
  assert.ok(body < sheet, "the prose comes before the sheet, so mounting cannot remove it");
  assert.match(html, /id="drawer"[^>]*tabindex="-1"/, "the drawer can take focus when it opens");
});

test("E closes the drawer from the button that opened it", () => {
  // After a click the `How it works` button holds focus. A focused button keeps
  // Space and the arrows — those are its own keys — but the drawer's own keys are
  // not, or the control that opens the drawer would make `E` dead.
  const key = (overrides) => ({
    key: "x", shiftKey: false, ctrlKey: false, metaKey: false, altKey: false, ...overrides,
  });
  assert.deepEqual(resolveKey(key({ key: "e", target: { tagName: "BUTTON" } })), { type: "explainer" });
  assert.deepEqual(resolveKey(key({ key: "E", target: { tagName: "BUTTON" } })), { type: "explainer" });
  assert.deepEqual(resolveKey(key({ key: "?", target: { tagName: "BUTTON" } })), { type: "keys" });
  assert.deepEqual(resolveKey(key({ key: "Escape", target: { tagName: "BUTTON" } })), { type: "close" });
  assert.equal(resolveKey(key({ key: " ", target: { tagName: "BUTTON" } })), null, "Space still clicks the button");
  assert.equal(resolveKey(key({ key: "e", target: { tagName: "INPUT" } })), null, "typing e stays typing");
});

// ── the gate itself ─────────────────────────────────────────────────────────

test("the word gate names the file, the line and the word", () => {
  const text = "first line\nsecond line with numpy in it\nthird line with an adaptive beam\n";
  const hits = scanText(text, "dist/assets/index-abc.js", { tiers: [TIER_A] });
  assert.deepEqual(
    hits.map((hit) => [hit.file, hit.line, hit.word]),
    [
      ["dist/assets/index-abc.js", 2, "numpy"],
      ["dist/assets/index-abc.js", 3, "adaptive beam"],
    ],
  );
  assert.match(formatHits(hits, 1), /dist\/assets\/index-abc\.js:2:\d+/);

  // A built artifact that does not exist is an error, not a silent pass.
  assert.throws(() => scanFile(join(WEB, "dist", "no-such-file.js")), /no-such-file/);

  // The same list, over a write and a read, is what makes the check a gate rather
  // than a paragraph: a banned word in a file is a hit with its byte line.
  const dir = mkdtempSync(join(tmpdir(), "words-"));
  try {
    const path = join(dir, "page.js");
    writeFileSync(path, "ok\nok\nstill numpy\n");
    const found = scanFile(path, { tiers: [TIER_A] });
    assert.equal(found.length, 1);
    assert.equal(found[0].line, 3);
    assert.equal(found[0].word, "numpy");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
