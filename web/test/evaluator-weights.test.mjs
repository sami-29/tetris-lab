// The shipped Evaluator's weights, as the page displays them — and the guard that
// keeps the page's copy honest.
//
// The Engine panel names the eight Features of the Evaluator `shipped()` returns and
// the integer Weight each one is charged at (§2.1.2: every figure labelled in words,
// with its unit and its provenance). Those numbers live in Rust
// (`engine/src/evaluator.rs`), and `web/src/core/` is the only JavaScript allowed to
// touch the artifact — there is no ABI export for the weights, because a weight is
// not needed to *solve* anything.
//
// So the page carries a table, and this test is the thing that stops the table
// drifting: it parses `engine/src/evaluator.rs` itself, asserts that the shipped
// Evaluator is the one the page names, and compares all eight Q16 integers. A tuning
// run that rewrites `E1_PUBLISHED` (ticket 07/08's verdict) fails here rather than
// shipping a page that quotes last week's weights.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { resolve } from "node:path";

import { Q16, SHIPPED_EVALUATOR, decimalOf, featureOf } from "../src/config/evaluator.ts";

const REPO = resolve(fileURLToPath(new URL(".", import.meta.url)), "..", "..");
const RUST = readFileSync(resolve(REPO, "engine", "src", "evaluator.rs"), "utf8");

/** `-Q16`, `Q16`, `-4 * Q16`, or a plain integer, as the Rust const writes it. */
function q16(text) {
  const value = text.trim();
  const scaled = value.match(/^(-?)(\d+)?\s*\*\s*Q16$/);
  if (scaled) return Number(`${scaled[1]}1`) * (scaled[2] === undefined ? 1 : Number(scaled[2])) * 65536;
  if (value === "Q16") return 65536;
  if (value === "-Q16") return -65536;
  return Number(value);
}

function rustTable(name) {
  const start = RUST.indexOf(`pub const ${name}: DtWeights = DtWeights {`);
  assert.notEqual(start, -1, `${name} is in evaluator.rs`);
  const body = RUST.slice(start, RUST.indexOf("};", start));
  const rows = [...body.matchAll(/^\s*(\w+):\s*([^,]+),/gm)].map((match) => [match[1], q16(match[2])]);
  return Object.fromEntries(rows);
}

test("the shipped Evaluator is the one whose weights the page shows", () => {
  // `shipped()` is the only route the ABI has to an Evaluator (evaluator.rs, "the
  // shipped one"). It returns E1 today; the page says E1, so the two agree.
  const shippedBody = RUST.slice(RUST.indexOf("pub fn shipped()"), RUST.indexOf("pub fn shipped()") + 400);
  assert.match(shippedBody, /SHIPPED_DT/, "E1 (Dellacherie–Thiery) is the shipped model");
  assert.equal(SHIPPED_EVALUATOR.model, "E1", "and the page names it");
});

test("all eight Weights on screen are the Rust integers, in the Rust's own order", () => {
  const rust = rustTable("E1_PUBLISHED");
  assert.equal(SHIPPED_EVALUATOR.features.length, 8, "eight Features, as E1 has");
  const keys = SHIPPED_EVALUATOR.features.map((feature) => feature.key);
  assert.deepEqual(keys, Object.keys(rust), "the page's rows are the Rust struct's fields, in order");
  for (const feature of SHIPPED_EVALUATOR.features) {
    assert.equal(feature.weightQ16, rust[feature.key], `${feature.key}: Q16 is the engine's integer`);
    assert.equal(feature.weightQ16 % Q16, 0, `${feature.key}: a whole unit, as the published table is`);
  }
});

test("a weight is shown as the decimal its integer divides to, and says so", () => {
  assert.equal(decimalOf(-65536), "-1.00");
  assert.equal(decimalOf(65536), "+1.00");
  assert.equal(decimalOf(-4 * 65536), "-4.00");
  assert.equal(decimalOf(0), "0.00");
  assert.equal(decimalOf(-51 * 65536), "-51.00");
  // The unit is the fixed-point integer, not the decimal: the decimal is a rendering.
  assert.match(featureOf("holes").tip, /fixed.point|Q16/i, "the tooltip says the integer is the source of truth");
  const rendered = SHIPPED_EVALUATOR.features.map((feature) => decimalOf(feature.weightQ16));
  assert.deepEqual(rendered, ["-1.00", "+1.00", "-1.00", "-1.00", "-4.00", "-1.00", "0.00", "0.00"],
    "the published table, as E1_PUBLISHED writes it");
});

test("every Feature carries the paper's words and its bound, so a bar has a scale", () => {
  const bounds = [[1, 20], [0, 16], [0, 220], [0, 210], [0, 200], [0, 840], [0, 1000], [0, 20]];
  SHIPPED_EVALUATOR.features.forEach((feature, index) => {
    assert.deepEqual([feature.bound[0], feature.bound[1]], bounds[index], `${feature.key}: the paper's bound`);
    assert.ok(feature.label.length > 2, `${feature.key}: a words label`);
    assert.ok(feature.paper.length > 12, `${feature.key}: the paper's own words, not a paraphrase`);
    assert.ok(feature.tip.includes(String(feature.bound[1])), `${feature.key}: the tooltip names its bound`);
  });
});
