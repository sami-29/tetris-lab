#!/usr/bin/env node
// The banned-words gate: search the BUILT page for the words ticket 16 forbids.
//
//   node scripts/check-words.mjs          # after `npm run build`
//
// Ticket 16: "It contains no mention of numpy, adaptive beam schedules, the Python
// app, Flask, or Render. Add a gate for that: a test or a gate check that searches
// the BUILT page for those words (and the glossary's avoided near-synonyms) and
// names the file and line on a hit."
//
// So the target is `dist/` — what the browser actually downloads — not the source:
//
//   * `index.html`            Tier A + Tier B (HTML comments stripped: a visitor
//                             cannot read one, and the shell documents itself there);
//   * `assets/*.js`           Tier A over the whole bundle, Tier B over its **string
//                             literals** only. A sentence a visitor can read is a
//                             literal; `event.target` is not the Goal;
//   * `assets/*.css`          Tier A. A stylesheet has no prose; `align-items: baseline`
//                             is not the Reference run, and the only words are `content:`
//                             strings, which the HTML and the bundle already cover;
//   * `assets/*.wasm`         Tier A, read as latin-1 so a Rust panic message cannot
//                             hide a word in the binary;
//   * `assets/*.js.map`       Tier A. The maps ship (`build.sourcemap`), so every
//                             `.ts` the page was built from is delivered inside them;
//   * `src/ui/explainer.ts`   Tier A + Tier B at real line numbers, because the
//                             drawer's prose is the one surface where a reviewer
//                             needs source coordinates and not a minified column.
//
// The word lists live in `scripts/words.mjs` and are shared with
// `test/explainer.test.mjs` and `scripts/browser-check.mjs`, so the three
// enforcement points cannot disagree.
//
// Exit 0 prints the files and sizes it scanned (a silent scan of zero files must be
// visible); exit 1 lists every hit as `file:line:column` with the words around it.

import { readFileSync, readdirSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join, relative } from "node:path";

import { TIER_A, TIER_B, formatHits, scanBundleStrings, scanFile, scanText } from "./words.mjs";

const HERE = fileURLToPath(new URL(".", import.meta.url));
const WEB = join(HERE, "..");
const DIST = join(WEB, "dist");
const ASSETS = join(DIST, "assets");

const rel = (path) => relative(WEB, path).replace(/\\/g, "/");

/** Replace HTML comments with spaces, keeping every newline where it was. */
function withoutComments(html) {
  return html.replace(/<!--[\s\S]*?-->/g, (comment) => comment.replace(/[^\n]/g, " "));
}

function listAssets() {
  try {
    return readdirSync(ASSETS).sort();
  } catch {
    return null;
  }
}

const assets = listAssets();
if (assets === null) {
  console.error(
    `✘ no dist/assets — run \`npm run build\` first (looked in ${rel(ASSETS)})`,
  );
  process.exit(1);
}

const files = [];
files.push({ path: join(DIST, "index.html"), bytes: statSync(join(DIST, "index.html")).size });
for (const name of assets) {
  const path = join(ASSETS, name);
  files.push({ path, bytes: statSync(path).size });
}

const hits = [];

for (const file of files) {
  const name = rel(file.path);
  if (name.endsWith(".js")) {
    const bundle = readFileSync(file.path, "latin1");
    hits.push(...scanText(bundle, name, { tiers: [TIER_A] }));
    hits.push(...scanBundleStrings(bundle, name, { tiers: [TIER_B] }));
  } else if (name.endsWith(".css")) {
    hits.push(...scanText(readFileSync(file.path, "latin1"), name, { tiers: [TIER_A] }));
  } else if (name.endsWith(".wasm") || name.endsWith(".map")) {
    hits.push(...scanFile(file.path, { tiers: [TIER_A] }));
  } else if (name.endsWith(".html")) {
    hits.push(...scanText(withoutComments(readFileSync(file.path, "latin1")), name, { tiers: [TIER_A, TIER_B] }));
  }
}

// The drawer's module, where the line numbers are real. Tier A only: the source is
// delivered inside the source map, so a hard token in a comment is a real leak, while
// the module's identifiers and import paths are not prose (Tier B is asserted on the
// drawer's rendered text, and on the bundle's string literals above).
const explainer = join(WEB, "src", "ui", "explainer.ts");
files.push({ path: explainer, bytes: statSync(explainer).size });
hits.push(...scanFile(explainer, { tiers: [TIER_A] }));

if (hits.length > 0) {
  console.error(formatHits(hits, files.length));
  console.error(`\nscanned ${files.length} files:`);
  for (const file of files) console.error(`  ${rel(file.path)}  ${file.bytes} B`);
  process.exit(1);
}

console.log(formatHits([], files.length));
for (const file of files) console.log(`  ${rel(file.path)}  ${file.bytes} B`);
