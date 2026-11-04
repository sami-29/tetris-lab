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

import { FIGURES, FIGURE_SOURCES, REQUIRED_FIGURES, STATIC_FIGURES } from "../src/ui/figure.ts";

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
  assert.match(css, /--fs-micro:\s*12px/, "the 12 px legibility floor on the smallest step");
  assert.match(css, /font-variant-numeric:\s*tabular-nums/, "tabular numerals, as the as-built page already had");
  assert.equal((css.match(/--fs-[a-z]+:\s*[\d.]+px/g) ?? []).length, 6, "six steps, no more");
});

test("the two accents are the palette's, and there is no third", () => {
  const tokens = read("src", "styles", "tokens.css");
  for (const colour of ["#e0a33e", "#3d8f8a", "#5fc0b8", "#c6ccd3", "#a5aeb6", "#8b959f", "#7d8893", "#0e1012", "#15181b", "#191d21"]) {
    assert.ok(tokens.includes(colour), `${colour} is a token`);
  }
  // One place owns colour. A hex in a component file is how a third accent appears.
  const known = new Set((tokens.match(/#[0-9a-f]{6}/g) ?? []).map((hex) => hex.toLowerCase()));
  const strangers = (css.match(/#[0-9a-f]{6}/g) ?? []).filter((hex) => !known.has(hex.toLowerCase()));
  assert.deepEqual(strangers, [], "shell.css names no colour the token sheet does not");
  // And the ceiling holds: amber and teal are the only saturated colours in the sheet.
  const saturated = [...known].filter((hex) => {
    const [r, g, b] = [1, 3, 5].map((at) => parseInt(hex.slice(at, at + 2), 16));
    return Math.max(r, g, b) - Math.min(r, g, b) > 40;
  });
  assert.deepEqual(saturated.sort(), ["#3d8f8a", "#5fc0b8", "#8a6524", "#e0a33e", "#f0b659"].sort(),
    "two accents and their two dim/bright steps, nothing else");
});

test("no emoji, no dingbats, no stock furniture: the icons are hand-made SVG", () => {
  // The notes' ban list (notes/14 §4.4): emoji, dingbats and the filled glyphs the
  // mockup's icons replace. Arrows stay allowed — a keyboard chip that says
  // "ArrowLeft" instead of "←" is worse — but not as decoration.
  const banned = /[\u{1F300}-\u{1FAFF}\u{2600}-\u{27BF}\u{2B00}-\u{2BFF}]/u;
  const replaced = ["\u25B6", "\u23F8", "\u23ED", "\u21BB", "\u2713", "\u2715", "\u25CF", "\u25A0", "\u25B2"];
  for (const file of [join(WEB, "index.html"), ...sources(join(WEB, "src"))]) {
    const text = readFileSync(file, "utf8");
    assert.equal(banned.test(text), false, `${file} carries a glyph that should be an SVG`);
    for (const glyph of replaced) {
      assert.equal(text.includes(glyph), false, `${file} carries ${JSON.stringify(glyph)}, which is an icon here`);
    }
  }
  // The icon set is the notes' eleven, each drawn here and not fetched. The ban is
  // on an *asset*: a file reference or a fetch. It is deliberately not `https?:`,
  // which would fire on the SVG namespace URI a hand-built `<svg>` legitimately
  // carries.
  const icons = read("src", "ui", "icons.ts");
  for (const name of ["brand-mark", "help", "play", "pause", "step-back", "step-forward",
    "restart", "close", "expert-chevron", "tree-toggle", "stop"]) {
    assert.ok(icons.includes(`"${name}"`) || icons.includes(`'${name}'`), `the ${name} icon exists`);
  }
  assert.doesNotMatch(icons, /<img|\.(png|jpe?g|gif|webp)\b|new URL\(|fetch\(|@import/, "hand-made, not an image or a font");
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
  // The static markup carries figures for the values that exist before a Solve —
  // now just the playback speed and the keyboard map, since the toolbar fields moved
  // into the Playground — so the first paint is never empty.
  const markup = [...html.matchAll(/data-figure="([a-z0-9-]+)"([^>]*)>/g)];
  assert.ok(markup.length >= 2, `the first paint has figures (${markup.length})`);
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
  // `beam-readout` and the preset buttons are built by ticket 13's Playground at boot,
  // so they are not in the static markup; everything below is.
  for (const id of ["board", "band", "strip", "caret", "play", "restart", "speed", "speed-readout",
    "status", "outcome", "progress", "sheet", "sheet-list", "playground", "band-label",
    "band-caption", "band-counts"]) {
    assert.ok(html.includes(`id="${id}"`), `#${id} is still on the page`);
  }
});

test("a static figure says exactly what the registry says it says", () => {
  // Two figures live in the markup so the first paint has words in it. They are still
  // *registry* figures: label, provenance, kind and tooltip must be the registry's, or
  // the page has two tables to keep in step and one of them is a lie.
  //
  // `STATIC_FIGURES` is the ceiling: a third duplicate is a test failure, not a
  // review note, so the cheap path for a new figure is `createFigure` at boot.
  const pasted = [...html.matchAll(/data-figure="([a-z0-9-]+)"/g)].map((match) => match[1]);
  assert.deepEqual([...pasted].sort(), [...STATIC_FIGURES].sort(),
    "the markup mirrors exactly the figures STATIC_FIGURES names");
  for (const [, id, attrs] of html.matchAll(/data-figure="([a-z0-9-]+)"([^>]*)>/g)) {
    const spec = FIGURES[id];
    assert.ok(spec, `${id} is a figure the registry knows`);
    const read = (name) => (attrs.match(new RegExp(`data-figure-${name}="([^"]*)"`)) ?? [])[1];
    assert.equal(read("label"), spec.label, `${id}: the label`);
    assert.equal(read("source"), spec.source, `${id}: the provenance`);
    assert.equal(read("kind"), spec.kind, `${id}: the kind`);
    const tip = (attrs.match(/data-tip="([^"]*)"/) ?? [])[1];
    assert.equal(tip, spec.tip, `${id}: the tooltip`);
  }
});
