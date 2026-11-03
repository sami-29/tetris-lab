// The words the shipped page may not say, and the scanner that finds them.
//
// Ticket 16's third bullet is a vocabulary rule, not a style note: the Explainer
// describes the engine that shipped, so the words of the engine it replaced — and
// the near-synonyms the glossary marks as avoided — must not reach a visitor. A
// reviewer cannot hold that in their head across a rewrite, so it is a list and a
// scan, run three times:
//
//   * `scripts/check-words.mjs` over the **built page** (dist), which is what the
//     browser actually downloads;
//   * `test/explainer.test.mjs` over the drawer's rendered text and its own source,
//     where the line numbers are real;
//   * `scripts/browser-check.mjs` over the drawer's `innerText` in a real browser,
//     because a template can assemble a word the static scan never sees.
//
// Two tiers, because a glossary's `_Avoid_` lists are semantic and a substring ban
// would fail on the page's own legitimate vocabulary (`column`, `Move`, `Run`,
// `score`, `bag`, `render` — the Canvas renderer). **Tier A** is unambiguous and
// blocking with no context. **Tier B** is a curated phrase list and is only applied
// to prose surfaces: the drawer's text, the static markup, and the string literals
// of the bundle (never to identifiers — `event.target` is not the Goal).
//
// Zero dependencies; every file is decoded latin-1 so a binary can never throw.

import { readFileSync } from "node:fs";

/** Tier A: hard tokens. No legitimate use on this page, so no context needed. */
export const TIER_A = [
  { word: "numpy", pattern: "\\bnumpy\\b", flags: "i" },
  { word: "num py", pattern: "\\bnum ?py\\b", flags: "i" },
  { word: "python app", pattern: "\\bpython app\\b", flags: "i" },
  { word: "flask app", pattern: "\\bflask app\\b", flags: "i" },
  { word: "python", pattern: "\\bc?python(?:3)?\\b", flags: "i" },
  { word: "flask", pattern: "\\bflask\\b", flags: "i" },
  { word: "htmx", pattern: "\\bhtmx\\b", flags: "i" },
  { word: "alpine", pattern: "\\balpine(?:\\.js)?\\b", flags: "i" },
  { word: "D3", pattern: "\\bD3\\b", flags: "" },
  { word: "multiprocessing", pattern: "\\bmultiprocessing\\b", flags: "i" },
  { word: "requirements.txt", pattern: "\\brequirements\\.txt\\b", flags: "i" },
  { word: "Procfile", pattern: "\\bprocfile\\b", flags: "i" },
  { word: "render.com", pattern: "\\brender\\.com\\b", flags: "i" },
  { word: "heroku", pattern: "\\bheroku\\b", flags: "i" },
  { word: "adaptive beam", pattern: "\\badaptive beam\\b", flags: "i" },
  { word: "progressive beam", pattern: "\\bprogressive beam\\b", flags: "i" },
  { word: "beam schedule", pattern: "\\bbeam schedul\\w*", flags: "i" },
  { word: "widths attempted", pattern: "\\bwidths? attempted\\b", flags: "i" },
  { word: "old solver", pattern: "\\bold solver\\b", flags: "i" },
  { word: "previous solver", pattern: "\\bprevious solver\\b", flags: "i" },
  { word: "coming soon", pattern: "\\bcoming soon\\b", flags: "i" },
];

/**
 * Tier B: the glossary's avoided near-synonyms, phrase-anchored.
 *
 * `node` is absent on purpose even though Evaluation avoids it: `node` is the tree
 * view's own word for a drawn state, and the ban is scoped to the drawer, where
 * this list still applies because the drawer's prose is one of the scanned surfaces.
 */
export const TIER_B = [
  // Lab: app, tool, demo, portfolio page
  { word: "the app", pattern: "\\bthe app\\b", flags: "i" },
  { word: "this tool", pattern: "\\bthis tool\\b", flags: "i" },
  { word: "demo", pattern: "\\bdemo\\b", flags: "i" },
  { word: "portfolio page", pattern: "\\bportfolio page\\b", flags: "i" },
  // Run: simulation, batch, job, sweep
  { word: "simulation", pattern: "\\bsimulation\\b", flags: "i" },
  { word: "batch", pattern: "\\bbatch\\b", flags: "i" },
  { word: "job", pattern: "\\bjob\\b", flags: "i" },
  { word: "sweep", pattern: "\\bsweep\\b", flags: "i" },
  // Dataset: corpus, benchmark set, sample set
  { word: "corpus", pattern: "\\bcorpus\\b", flags: "i" },
  { word: "benchmark set", pattern: "\\bbenchmark set\\b", flags: "i" },
  { word: "sample set", pattern: "\\bsample set\\b", flags: "i" },
  // Split: subset, partition, half, fold
  { word: "subset", pattern: "\\bsubset\\b", flags: "i" },
  { word: "partition", pattern: "\\bpartition\\b", flags: "i" },
  { word: "half", pattern: "\\bhalf\\b", flags: "i" },
  { word: "fold", pattern: "\\bfold\\b", flags: "i" },
  // Reference run: baseline, leaderboard, benchmark run
  { word: "baseline", pattern: "\\bbaseline\\b", flags: "i" },
  { word: "leaderboard", pattern: "\\bleaderboard\\b", flags: "i" },
  { word: "benchmark run", pattern: "\\bbenchmark run\\b", flags: "i" },
  // Playground: editor, settings panel, config
  { word: "editor", pattern: "\\beditor\\b", flags: "i" },
  { word: "settings panel", pattern: "\\bsettings panel\\b", flags: "i" },
  { word: "config", pattern: "\\bconfig\\b", flags: "i" },
  // Explainer: docs, wiki, how-it-works page
  { word: "the docs", pattern: "\\bthe docs\\b", flags: "i" },
  { word: "wiki", pattern: "\\bwiki\\b", flags: "i" },
  { word: "how-it-works page", pattern: "\\bhow-it-works page\\b", flags: "i" },
  // Dataset fingerprint: checksum, digest
  { word: "checksum", pattern: "\\bchecksum\\b", flags: "i" },
  { word: "digest", pattern: "\\bdigest\\b", flags: "i" },
  // Ablation: experiment, A/B, variant test
  { word: "experiment", pattern: "\\bexperiment\\b", flags: "i" },
  { word: "A/B", pattern: "\\bA\\/B\\b", flags: "" },
  { word: "variant test", pattern: "\\bvariant test\\b", flags: "i" },
  // Width cap: max width, width limit, width ceiling
  { word: "max width", pattern: "\\bmax(?:imum)? width\\b", flags: "i" },
  { word: "width limit", pattern: "\\bwidth limit\\b", flags: "i" },
  { word: "width ceiling", pattern: "\\bwidth ceiling\\b", flags: "i" },
  // Board: the grid, the matrix, playfield
  { word: "the grid", pattern: "\\bthe grid\\b", flags: "i" },
  { word: "the matrix", pattern: "\\bthe matrix\\b", flags: "i" },
  { word: "playfield", pattern: "\\bplayfield\\b", flags: "i" },
  // Column word: bitmap, mask
  { word: "bitmap", pattern: "\\bbitmap\\b", flags: "i" },
  { word: "mask", pattern: "\\bmask\\b", flags: "i" },
  // Sequence: the queue, the bag
  { word: "the queue", pattern: "\\bthe queue\\b", flags: "i" },
  { word: "the bag", pattern: "\\bthe bag\\b", flags: "i" },
  // Goal: target, threshold
  { word: "target", pattern: "\\btarget\\b", flags: "i" },
  { word: "threshold", pattern: "\\bthreshold\\b", flags: "i" },
  // Beam width, Layer
  { word: "beam depth", pattern: "\\bbeam depth\\b", flags: "i" },
  { word: "beam budget", pattern: "\\bbeam budget\\b", flags: "i" },
  { word: "beam level", pattern: "\\bbeam (?:level|ply|generation)\\b", flags: "i" },
  // Dedup: transposition table, prune, merge
  { word: "transposition table", pattern: "\\btransposition table\\b", flags: "i" },
  { word: "prune", pattern: "\\bprune\\b", flags: "i" },
  { word: "merge", pattern: "\\bmerge\\b", flags: "i" },
  // Tail search, No-suicide rule
  { word: "endgame", pattern: "\\bendgame\\b", flags: "i" },
  { word: "safe move", pattern: "\\bsafe move\\b", flags: "i" },
  { word: "game-over check", pattern: "\\bgame-over check\\b", flags: "i" },
  // Solve: attempt, trial
  { word: "attempt", pattern: "\\battempt\\b", flags: "i" },
  { word: "trial", pattern: "\\btrial\\b", flags: "i" },
  // Evaluator, Feature, Weight, Evaluation
  { word: "heuristic", pattern: "\\bheuristic\\b", flags: "i" },
  { word: "fitness function", pattern: "\\bfitness function\\b", flags: "i" },
  { word: "objective function", pattern: "\\bobjective function\\b", flags: "i" },
  { word: "metric", pattern: "\\bmetric\\b", flags: "i" },
  { word: "coefficient", pattern: "\\bcoefficient\\b", flags: "i" },
  { word: "parameter", pattern: "\\bparameter\\b", flags: "i" },
  { word: "sample", pattern: "\\bsample\\b", flags: "i" },
  { word: "iteration", pattern: "\\biteration\\b", flags: "i" },
];

const CONTEXT = 60;

/** A pattern as a fresh global regex — `lastIndex` must not leak between files. */
function regexOf(entry) {
  return new RegExp(entry.pattern, `${entry.flags}g`);
}

/**
 * Scan one string and return every hit, with its line and column.
 *
 * `file` is only a label (the caller knows the file); the positions are relative to
 * the string, so a caller that maps a literal back into a bundle passes the literal
 * and adjusts with `offset`.
 *
 * Overlapping matches are reported once: a text that says "python app" is not two
 * findings, and a gate whose count is wrong is a gate people stop trusting.
 */
export function scanText(text, file, { tiers = [TIER_A], offset = 0, lineBase = 1 } = {}) {
  const hits = [];
  const lines = text.split("\n");
  for (const tier of tiers) {
    for (const entry of tier) {
      const regex = regexOf(entry);
      lines.forEach((line, index) => {
        for (const match of line.matchAll(regex)) {
          hits.push({
            file,
            line: lineBase + index,
            column: match.index + 1,
            index: offset + match.index,
            length: match[0].length,
            word: entry.word,
            text: match[0],
            context: line.trim().slice(Math.max(0, match.index - CONTEXT), match.index + match[0].length + CONTEXT),
          });
        }
      });
    }
  }
  // One hit per overlapping span, in reading order.
  hits.sort((a, b) => a.line - b.line || a.column - b.column || b.length - a.length);
  const kept = [];
  for (const hit of hits) {
    const overlaps = kept.some(
      (other) => other.line === hit.line && hit.column < other.column + other.length && other.column < hit.column + hit.length,
    );
    if (!overlaps) kept.push(hit);
  }
  return kept;
}

/** Read one file as latin-1 (never throws on binary) and scan it. */
export function scanFile(path, options = {}) {
  return scanText(readFileSync(path).toString("latin1"), path, options);
}

/** The message a human reads: file, line, column, word and the words around it. */
export function formatHits(hits, fileCount = 1) {
  if (hits.length === 0) return `no banned words in ${fileCount} file${fileCount === 1 ? "" : "s"}`;
  const lines = hits.map(
    (hit) =>
      `✘ banned word "${hit.word}"   ${hit.file}:${hit.line}:${hit.column}   …${hit.context}…`,
  );
  const files = new Set(hits.map((hit) => hit.file));
  lines.push(`${hits.length} hit${hits.length === 1 ? "" : "s"} in ${files.size} file${files.size === 1 ? "" : "s"}`);
  return lines.join("\n");
}

/**
 * The string literals of a minified bundle, with their offset in the bundle.
 *
 * This is how Tier B reaches the built page without reading identifiers: every
 * tooltip, label and sentence the page can show is a string literal, and
 * `event.target` is not.
 */
export function stringLiterals(bundle) {
  const found = [];
  for (const pattern of [/"(?:[^"\\]|\\.)*"/g, /`(?:[^`\\]|\\.)*`/g]) {
    for (const match of bundle.matchAll(pattern)) {
      found.push({ text: match[0].slice(1, -1), offset: match.index + 1 });
    }
  }
  found.sort((a, b) => a.offset - b.offset);
  return found;
}

/** The 1-based line and column of one offset in `text`. */
function positionOf(text, index) {
  const before = text.slice(0, index);
  const line = before.split("\n").length;
  return { line, column: index - before.lastIndexOf("\n") };
}

/**
 * Tier B over the built bundle's prose: its string literals, reported at the
 * bundle's own line and column — the coordinates a reviewer opens the file at.
 */
export function scanBundleStrings(bundle, file, { tiers = [TIER_B] } = {}) {
  const hits = [];
  for (const literal of stringLiterals(bundle)) {
    const local = scanText(literal.text, file, { tiers });
    for (const hit of local) {
      const index = literal.offset + (hit.line === 1 ? hit.column - 1 : 0);
      hits.push({ ...hit, ...positionOf(bundle, index) });
    }
  }
  return hits;
}
