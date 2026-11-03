#!/usr/bin/env node
// dist/manifest.json — the generated map from a logical name to its hashed path.
//
//   node scripts/manifest.mjs          # writes dist/manifest.json (after `vite build`)
//
// §10: "every asset carries a content hash ... and a generated `manifest.json` maps
// logical name -> hashed path. No service worker, no `?v=` strings, no
// hand-maintained versions." Vite does the hashing (the wasm too — `main.ts`
// imports it with `?url`, and `assetsInlineLimit: 0` keeps it a file); this script
// is what turns the built directory into a name a human and the post-deploy smoke
// test can address: "the wasm" is `engine.wasm` here, `engine-BO4_EbTC.wasm` on the
// wire.
//
// Three properties, each of which the ticket's acceptance depends on:
//
//   1. **The hash comes off the stem, not the whole name.** `index-CvTXBxIA.js.map`
//      is `index.js.map`; stripping every `-<8 chars>` segment would eat the `.map`
//      and collide with the chunk's own entry.
//   2. **Nothing volatile.** No build time, no commit, no counter. A rebuild that
//      changes nothing must produce the same hashes *and* the same manifest, or
//      every no-op deploy republishes a file that says the payload moved.
//   3. **Fail closed.** A file the generator cannot name, an asset nothing
//      references, or a reference to a file that was not emitted is an error with
//      the path in the message. The build stops here, before an upload exists that
//      a visitor could land on.
//
// Exit codes: 0 wrote the manifest, 1 the tree is not shippable, 2 nothing to read.

import { createHash } from "node:crypto";
import { existsSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const WEB = join(HERE, "..");
const DIST = join(WEB, "dist");

/** Bumped only if the shape below changes; the smoke test reads `schema`. */
export const SCHEMA = 1;

/**
 * Rollup/`vite`'s `[hash]` is 8 characters of base64url — `-` and `_` included, so
 * the segment boundary has to be found by the lookahead, not by `[a-z0-9]`.
 */
const HASH = /-[A-Za-z0-9_-]{8}(?=(\.[A-Za-z0-9]+)+$)/;

/** Files the platform consumes and never serves: uploaded, not addressable. */
const PASSTHROUGH = new Set(["_headers", "_redirects", "manifest.json"]);

/** The payload §9.3 counts. Source maps are published but are not the page. */
const PAYLOAD = new Set([".html", ".js", ".css", ".wasm"]);

/** Assets whose text names other assets: the reference graph is walked through these. */
const TEXTUAL = new Set([".html", ".js", ".css"]);

const extensionOf = (path) => {
  const name = path.slice(path.lastIndexOf("/") + 1);
  const dot = name.lastIndexOf(".");
  return dot === -1 ? "" : name.slice(dot);
};

/** `assets/index-CvTX_Bx-.js` -> `index.js`; a file with no hash is its own name. */
export function logicalName(rel) {
  const slash = rel.lastIndexOf("/");
  const dir = rel.slice(0, slash + 1);
  const stem = rel.slice(slash + 1).replace(HASH, "");
  return `${dir}${stem}`.replace(/^assets\//, "");
}

function walk(dir) {
  const out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walk(path));
    else if (entry.isFile()) out.push(path);
  }
  return out;
}

const toPosix = (root, path) => relative(root, path).split(sep).join("/");

/**
 * Read `dist/`, name every file, and check the reference graph.
 *
 * `dist` is a parameter rather than a constant so the tests can run the generator
 * over a fixture tree; the caller's default is the real build.
 */
export function buildManifest(dist = DIST) {
  if (!existsSync(dist) || !statSync(dist).isDirectory()) {
    throw new Error(`${dist} is not a directory — run \`npm run build\` first`);
  }
  const files = walk(dist).map((path) => toPosix(dist, path)).sort();
  if (!files.includes("index.html")) {
    throw new Error(`${dist} holds no index.html — run \`npm run build\` first`);
  }

  const assets = new Map();
  for (const rel of files) {
    const name = rel.slice(rel.lastIndexOf("/") + 1);
    if (PASSTHROUGH.has(name)) continue;
    if (rel !== "index.html" && !HASH.test(rel)) {
      throw new Error(
        `dist holds a file the manifest cannot name: ${rel} ` +
          `(no -<8 char hash> segment; Vite's hash length or the asset layout changed)`,
      );
    }
    const logical = logicalName(rel);
    if (assets.has(logical)) {
      throw new Error(
        `two built files share the logical name ${logical}: ${assets.get(logical).path} and /${rel}`,
      );
    }
    const bytes = readFileSync(join(dist, rel));
    assets.set(logical, {
      path: `/${rel}`,
      bytes: bytes.length,
      sha256: createHash("sha256").update(bytes).digest("hex"),
    });
  }

  // ── the reference graph: index.html -> the chunks it names -> what they name ──
  const paths = new Set([...assets.values()].map((asset) => asset.path));
  const byPath = new Map([...assets.entries()].map(([logical, asset]) => [asset.path, logical]));

  const indexHtml = readFileSync(join(dist, "index.html"), "utf8");
  const referenced = [...indexHtml.matchAll(/(?:src|href)="(\/[^"]*)"/g)].map((match) => match[1]);
  for (const ref of referenced) {
    if (!paths.has(ref)) {
      throw new Error(`index.html names ${ref}, which the build did not emit`);
    }
  }

  const mentions = (text, path) => text.includes(path) || text.includes(path.slice(1));
  const reached = new Set(["index.html"]);
  const queue = ["index.html"];
  while (queue.length > 0) {
    const logical = queue.shift();
    const asset = assets.get(logical);
    if (!TEXTUAL.has(extensionOf(asset.path))) continue;
    const text = readFileSync(join(dist, asset.path.slice(1)), "utf8");
    for (const [path, other] of byPath) {
      if (reached.has(other) || !mentions(text, path)) continue;
      reached.add(other);
      queue.push(other);
    }
  }
  for (const [logical, asset] of assets) {
    if (!PAYLOAD.has(extensionOf(asset.path)) || reached.has(logical)) continue;
    throw new Error(
      `nothing on the page references ${asset.path} (${logical}): ` +
        `an emitted payload no request can reach is a build bug, not a cache entry`,
    );
  }

  const ordered = {};
  for (const logical of [...assets.keys()].sort()) ordered[logical] = assets.get(logical);
  return { schema: SCHEMA, assets: ordered };
}

/** The bytes written to disk: stable, sorted, and with no room for a timestamp. */
export function manifestText(manifest) {
  return `${JSON.stringify(manifest, null, 2)}\n`;
}

function main() {
  let manifest;
  try {
    manifest = buildManifest(DIST);
  } catch (error) {
    console.error(`error: ${error.message}`);
    process.exit(1);
  }
  const out = join(DIST, "manifest.json");
  writeFileSync(out, manifestText(manifest));
  const assets = Object.entries(manifest.assets);
  console.log(`manifest.json · ${assets.length} assets`);
  for (const [logical, asset] of assets) {
    console.log(`  ${logical.padEnd(20)} ${asset.path.padEnd(34)} ${asset.bytes} B`);
  }
  console.log(`  -> ${out}`);
}

// Importable (the tests do) and runnable (the build does).
if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main();
