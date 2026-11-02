// The workbench shell, asserted where it is *mechanical*.
//
// The build spec is explicit that this ticket's visual requirements — motion,
// typography, the tooltip-every-figure rule and the large-screen layout — are
// judged against §2.5 by eye and by the headless acceptance pass, not asserted in a
// unit test ("a test that a tooltip exists would pass on a page nobody wants to look
// at"). So this file asserts the things that *can* be checked without pretending to
// judge taste:
//
//   * the five regions of §2 exist, in document order, and the grid arithmetic that
//     makes a no-scroll page is in the stylesheet rather than in a comment;
//   * every control in the static markup carries a tooltip;
//   * the figure registry covers the ticket's table — every figure the screen shows
//     has a words label, a unit, a provenance and a sentence of explanation, and the
//     one figure the ABI cannot answer (`expanded`, notes/14 C14) is absent;
//   * no emoji and no dingbat glyphs: the icons are hand-made SVG (§2.1.3);
//   * the ids the headless harness drives (`web/scripts/browser-check.mjs`) survive
//     the shell being replaced.
//
// What this file does not do: check that a label is *true*, that a tooltip is
// *useful*, or that 2560×1440 looks deliberate. `scripts/shell-check.mjs` asserts
// the structure in a real browser; the report names what is left to the eye.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join, resolve } from "node:path";

import { FIGURES, FIGURE_SOURCES, REQUIRED_FIGURES } from "../src/ui/figure.ts";

const WEB = resolve(fileURLToPath(new URL(".", import.meta.url)), "..");
const read = (...parts) => readFileSync(join(WEB, ...parts), "utf8");

const html = read("index.html");
const css = [read("src", "styles", "tokens.css"), read("src", "styles", "shell.css")].join("\n");

/** Every file under `src/`, so the ban list is a sweep and not a spot check. */
function sources(dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) out.push(...sources(path));
    else if (/\.(ts|css|html)$/.test(name)) out.push(path);
  }
  return out;
}

test("the five regions of §2 exist, in document order", () => {
  const order = [...html.matchAll(/data-region="([a-z]+)"/g)].map((match) => match[1]);
  assert.deepEqual(order, ["toolbar", "rail", "centre", "inspector", "status"], "top to bottom, one each");
});

test("the layout arithmetic is in the stylesheet: three tracks, one flexible row and column", () => {
  // The notes' §1.1 contract. The inspector is 420–620 px, *not* the mockup's 326:
  // at 326 the per-layer pitch falls under the 14 px floor ticket 12 sets.
  assert.match(css, /grid-template-columns:\s*236px\s+minmax\(0,\s*1fr\)\s+clamp\(420px,\s*24vw,\s*620px\)/,
    "rail 236 · centre 1fr · inspector clamp(420px, 24vw, 620px)");
  assert.match(css, /grid-template-rows:\s*48px\s+minmax\(0,\s*1fr\)\s+28px/, "toolbar 48 · stage 1fr · status 28");
  assert.match(css, /grid-template-areas:\s*"bar bar bar"\s+"rail stage inspect"\s*"status status status"/,
    "the regions are the grid, so nothing floats");
  // No page scroll. `html, body { overflow: hidden }` plus `height: 100%` and a
  // `100vh` app frame is what makes it true by construction.
  assert.match(css, /html,\s*body\s*\{[^}]*overflow:\s*hidden/, "the document does not scroll");
  assert.match(css, /html,\s*body\s*\{[^}]*height:\s*100%/, "and it is as tall as the viewport");
  assert.match(css, /#app\s*\{[^}]*height:\s*100vh/, "the app frame grows into the viewport height");
  // Every flexible child carries `min-*: 0`, or a canvas' intrinsic size wins and the
  // page scrolls anyway. One rule, applied to the tracks that can overflow.
  assert.match(css, /\[data-region\]\s*\{[^}]*min-height:\s*0/, "regions can shrink");
  assert.match(css, /\[data-region\]\s*\{[^}]*min-width:\s*0/, "… in both directions");
});

test("the type scale is six steps and every numeral is tabular", () => {
  for (const token of ["--fs-micro", "--fs-chip", "--fs-data", "--fs-body", "--fs-title", "--fs-brand"]) {
    assert.ok(css.includes(token), `${token} is part of the scale`);
  }
  assert.match(css, /--fs-micro:\s*9px/, "the notes' 9 px micro label");
  assert.match(css, /font-variant-numeric:\s*tabular-nums/, "tabular numerals, as the as-built page already had");
  assert.equal((css.match(/--fs-[a-z]+:\s*[\d.]+px/g) ?? []).length, 6, "six steps, no more");
});

test("the two accents are the palette's, and there is no third", () => {
  for (const colour of ["#e0a33e", "#3d8f8a", "#5fc0b8", "#c6ccd3", "#8b959f", "#5e6873", "#454e57", "#0e1012", "#15181b", "#191d21"]) {
    assert.ok(css.includes(colour), `${colour} is a token`);
  }
  const hexes = new Set((css.match(/#[0-9a-f]{6}/g) ?? []).map((hex) => hex.toLowerCase()));
  const allowed = new Set(["#e0a33e", "#3d8f8a", "#5fc0b8", "#c6ccd3", "#8b959f", "#5e6873", "#454e57",
    "#0e1012", "#15181b", "#191d21", "#23282d", "#2c3238", "#8a6524", "#101315", "#000000", "#0b0f14", "#ffffff"]);
  const strangers = [...hexes].filter((hex) => !allowed.has(hex));
  assert.deepEqual(strangers, [], "no colour outside the ink ramp, the two accents and the board's own palette");
});

test("no emoji, no dingbats, no stock furniture: the icons are hand-made SVG", () => {
  const banned = /[\u{1F300}-\u{1FAFF}\u{2600}-\u{27BF}\u{2B00}-\u{2BFF}\u{2190}-\u{21FF}\u{25A0}-\u{25FF}]/u;
  for (const file of [join(WEB, "index.html"), ...sources(join(WEB, "src"))]) {
    const text = readFileSync(file, "utf8");
    assert.equal(banned.test(text), false, `${file} carries a glyph that should be an SVG`);
    assert.doesNotMatch(text, /\bemoji\b/i, `${file} talks about emoji`);
  }
  // The icon set is the notes' eleven, each drawn here and not fetched.
  const icons = read("src", "ui", "icons.ts");
  for (const name of ["brand-mark", "help", "play", "pause", "step-back", "step-forward",
    "restart", "close", "expert-chevron", "tree-toggle", "stop"]) {
    assert.ok(icons.includes(`"${name}"`) || icons.includes(`'${name}'`), `the ${name} icon exists`);
  }
  assert.doesNotMatch(icons, /<img|\.png|\.svg"|https?:/, "hand-made, not an image or a font");
});

test("every control in the markup carries a tooltip", () => {
  const controls = [...html.matchAll(/<(button|input|select|a)\b([^>]*)>/g)];
  assert.ok(controls.length >= 6, `the shell has controls to check (${controls.length} found)`);
  const untipped = controls.filter(([, , attrs]) => !/data-tip="/.test(attrs)).map(([, tag, attrs]) => `${tag} ${attrs.slice(0, 40)}`);
  assert.deepEqual(untipped, [], "a control without a tooltip is a defect (§2.1.2)");
});

test("the figure registry covers the ticket's table, with a label, a unit and a provenance each", () => {
  const allowed = new Set(FIGURE_SOURCES);
  const bad = [];
  for (const id of REQUIRED_FIGURES) {
    const spec = FIGURES[id];
    if (!spec) {
      bad.push(`${id}: not in the registry`);
      continue;
    }
    if (!spec.label || spec.label.trim().length < 3) bad.push(`${id}: no words label`);
    if (!allowed.has(spec.source)) bad.push(`${id}: source ${spec.source} is not one of ${FIGURE_SOURCES.join("|")}`);
    if (spec.kind === "number" && !spec.unit) bad.push(`${id}: a number without a unit`);
    if (!spec.tip || spec.tip.trim().length < 24) bad.push(`${id}: no sentence of explanation`);
    if (spec.tip === spec.label) bad.push(`${id}: the tooltip repeats the label`);
  }
  assert.deepEqual(bad, [], `${REQUIRED_FIGURES.length} figures, each labelled in words`);
  // The ABI has no per-Layer candidate count (§4.4). The mockup's fabricated
  // "expanded" figure is deleted, not renamed: a registry entry for it would be a
  // number no engine produced.
  assert.equal(FIGURES.c14, undefined, "no per-Layer 'expanded' figure: the ABI cannot answer it");
});

test("a figure element is data-figure + label + source, and never a bare dash", () => {
  // The static markup carries figures for the values that exist before a Solve, so
  // the first paint is never empty (notes/14 §3).
  const markup = [...html.matchAll(/data-figure="([a-z0-9-]+)"([^>]*)>/g)];
  assert.ok(markup.length >= 4, `the first paint has figures (${markup.length})`);
  for (const [, id, attrs] of markup) {
    assert.match(attrs, /data-figure-label="[^"]{3,}"/, `${id} is labelled in words`);
    assert.match(attrs, /data-figure-source="(solve|trace|replay|dataset|reference|url|measured|control)"/, `${id} says where it came from`);
  }
});

test("the empty-region rule: every region carries static skeleton words", () => {
  for (const region of ["toolbar", "rail", "centre", "inspector", "status"]) {
    const start = html.indexOf(`data-region="${region}"`);
    assert.notEqual(start, -1, `${region} exists`);
    const body = html.slice(start, html.indexOf("data-region=", start + 1) === -1 ? undefined : html.indexOf("data-region=", start + 1));
    const words = body.replace(/<[^>]+>/g, " ").replace(/\s+/g, " ").trim();
    assert.ok(words.length > 40, `${region} has words on the page before any Solve (${words.length} chars)`);
  }
});

test("the Explainer drawer and its scrim exist, and the keyboard sheet lives inside", () => {
  assert.match(html, /id="drawer"[^>]*data-open="false"/, "the drawer is closed on load");
  assert.match(html, /id="scrim"/, "and has a scrim to close it by");
  assert.match(html, /id="sheet"[^>]*data-open="false"/, "the keyboard sheet is a section of the drawer");
  assert.match(html, /How it works/, "the drawer's control is named in the spec's words");
});

test("the ids the headless harness drives survive the shell being replaced", () => {
  // notes/14 §10: keep `#status`/`#outcome`/`#progress` as slots so the ticket-01
  // harness keeps passing while the new assertions land.
  for (const id of ["board", "band", "strip", "caret", "play", "restart", "speed", "speed-readout",
    "status", "outcome", "progress", "sheet", "sheet-list", "playground", "band-label",
    "band-caption", "band-counts", "beam-readout"]) {
    assert.ok(html.includes(`id="${id}"`), `#${id} is still on the page`);
  }
});
