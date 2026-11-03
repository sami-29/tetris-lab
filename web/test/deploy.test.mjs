// Seam: the deploy pipeline, the headers, and the two smoke scripts.
//
// Ticket 17 puts the page on Cloudflare Workers Static Assets through GitHub
// Actions. Almost all of that happens somewhere this repository cannot see: a
// workflow runs on a runner, `wrangler` talks to an API, and the edge answers
// requests. What *can* be asserted here is the set of files the pipeline is made
// of, and the invariants in them that the acceptance criteria name:
//
//   * the manifest the build writes — logical name -> hashed path, hash stripped
//     from the stem (not the whole name), no volatile field, byte-stable across
//     two runs, and fail-closed on an unreferenced asset or a dangling reference;
//   * `_headers` — the wasm MIME, `immutable` for `/assets/*`, `must-revalidate`
//     for the page, and exactly one rule per URL family setting `Cache-Control`
//     (two matching rules are comma-joined, which would corrupt `immutable`);
//   * `wrangler.jsonc` — assets-only, no `main`, no Cloudflare-side `build`, and
//     `workers_dev`/`preview_urls` written out because a `routes` entry silently
//     turns both off;
//   * `deploy.yml` — a green `gates` run on `main` and nothing else deploys, the
//     hand-triggered preview uploads a version instead, two repository secrets and
//     no rollback;
//   * the smoke scripts — the assertion list §10 names, in the shape
//     `host-probe/verify.sh` proved on this account.
//
// What this file cannot do, and does not pretend to: it cannot prove the workflow
// runs, that the token is sufficient, that the edge sends these headers, or that a
// browser instantiates the deployed wasm. Those are the rows of ticket 17's report
// that read "not observed". The mechanical half is here so a rename, a dropped
// `immutable`, a third secret or a `rollback` step lands in a red test instead of
// in a deploy.
//
//   node --test "test/**/*.test.mjs"        (from web/)

import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { buildManifest, logicalName } from "../scripts/manifest.mjs";

const HERE = fileURLToPath(new URL(".", import.meta.url));
const WEB = fileURLToPath(new URL("../", import.meta.url));
const ROOT = fileURLToPath(new URL("../../", import.meta.url));
const DIST = join(WEB, "dist");

const read = (path) => readFileSync(path, "utf8");
const readIf = (path) => (existsSync(path) ? readFileSync(path, "utf8") : null);

// ─────────────────────────────────────────────────────────── a fixture dist tree

/**
 * A miniature `dist/` with the five shapes Vite emits, so the generator is
 * exercised without a build. The hashes are 8 characters of base64url *including*
 * `-` and `_`, which is what Rollup actually produces and what the strip regex has
 * to survive.
 */
const HASHES = {
  index: "CvTX_Bx-",
  css: "BgWENkHr",
  engine: "BO4_EbTC",
  worker: "DZSqr1HT",
};

function fixture(files) {
  const dir = mkdtempSync(join(tmpdir(), "tetris-dist-"));
  for (const [rel, body] of Object.entries(files)) {
    const path = join(dir, rel);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, body);
  }
  return dir;
}

/** The tree a clean build of the page produces, as text, for the generator tests. */
function goodDist() {
  const index = `/assets/index-${HASHES.index}.js`;
  const css = `/assets/index-${HASHES.css}.css`;
  return fixture({
    "index.html": `<!doctype html><script type="module" src="${index}"></script><link rel="stylesheet" href="${css}">`,
    // The real entry chunk names both: the wasm it `?url`-imports and the worker it
    // spawns. Nothing in the fixture is unreachable by accident.
    [`assets/index-${HASHES.index}.js`]:
      `const wasm = new URL("/assets/engine-${HASHES.engine}.wasm", location.href);` +
      `const worker = new URL("/assets/solve.worker-${HASHES.worker}.js", location.href);` +
      `//# sourceMappingURL=index-${HASHES.index}.js.map`,
    [`assets/index-${HASHES.index}.js.map`]: `{"version":3}`,
    [`assets/index-${HASHES.css}.css`]: `:root{--ink:#0b0f14}`,
    [`assets/engine-${HASHES.engine}.wasm`]: new Uint8Array([0x00, 0x61, 0x73, 0x6d, 1, 0, 0, 0]),
    [`assets/solve.worker-${HASHES.worker}.js`]: `importScripts();`,
    [`assets/solve.worker-${HASHES.worker}.js.map`]: `{"version":3}`,
    _headers: `# copied from public/\n/*\n  X-Content-Type-Options: nosniff\n`,
  });
}

const withDist = (files, body) => {
  const dir = goodDist();
  for (const [rel, text] of Object.entries(files ?? {})) {
    const path = join(dir, rel);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, text);
  }
  try {
    return body(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
};

// ─────────────────────────────────────────────────────── the manifest (hashing)

test("the hash comes off the stem, and a source map keeps its own name", () => {
  assert.equal(logicalName(`assets/index-${HASHES.index}.js`), "index.js");
  assert.equal(logicalName(`assets/index-${HASHES.index}.js.map`), "index.js.map");
  assert.equal(logicalName(`assets/index-${HASHES.css}.css`), "index.css");
  assert.equal(logicalName(`assets/engine-${HASHES.engine}.wasm`), "engine.wasm");
  assert.equal(logicalName(`assets/solve.worker-${HASHES.worker}.js`), "solve.worker.js");
  assert.equal(logicalName("index.html"), "index.html");
  // An unhashed file is named as itself: nothing silently loses its last segment.
  assert.equal(logicalName("favicon.ico"), "favicon.ico");
});

test("every built file is named, and the map is logical name -> hashed path", () => {
  withDist({}, (dist) => {
    const manifest = buildManifest(dist);
    assert.equal(manifest.schema, 1);
    assert.deepEqual(Object.keys(manifest.assets).sort(), [
      "engine.wasm",
      "index.css",
      "index.html",
      "index.js",
      "index.js.map",
      "solve.worker.js",
      "solve.worker.js.map",
    ]);
    assert.equal(manifest.assets["engine.wasm"].path, `/assets/engine-${HASHES.engine}.wasm`);
    assert.equal(manifest.assets["engine.wasm"].bytes, 8);
    assert.match(manifest.assets["engine.wasm"].sha256, /^[0-9a-f]{64}$/);
    assert.equal(manifest.assets["index.html"].path, "/index.html");
    // `_headers` is plumbing, not an asset: it is uploaded and never served.
    assert.equal(manifest.assets._headers, undefined);
    assert.equal(manifest.assets["manifest.json"], undefined);
  });
});

test("the manifest holds nothing that moves when the bytes did not", () => {
  withDist({}, (dist) => {
    const first = buildManifest(dist);
    const second = buildManifest(dist);
    assert.deepEqual(first, second);
    // No timestamp, no commit, no build counter: a rebuild that changes nothing
    // must republish the same manifest, or every no-op deploy looks different.
    const text = JSON.stringify(first);
    assert.doesNotMatch(text, /"(built|date|time|timestamp|commit|sha|version|revision)"/i);
    assert.doesNotMatch(text, /\d{4}-\d{2}-\d{2}T/);
    // Sorted, so the file's bytes do not depend on the order the directory listed.
    assert.deepEqual(Object.keys(second.assets), Object.keys(first.assets).sort());
  });
});

test("an asset nothing references, and a reference to nothing, both fail loudly", () => {
  // The orphan: a file the page cannot reach means the build emitted a payload the
  // visitor never asks for — usually a chunk that lost its importer.
  withDist({ "assets/orphan-ZZZZZZZZ.js": "export const unused = 1;" }, (dist) => {
    assert.throws(() => buildManifest(dist), /orphan-ZZZZZZZZ\.js/);
  });
  // The dangling reference: the page names a file the build did not emit. This is
  // the half-deploy the ticket's atomicity criterion exists to prevent.
  withDist({}, (dist) => {
    const path = join(dist, "index.html");
    writeFileSync(
      path,
      `<script type="module" src="/assets/index-${HASHES.index}.js"></script>` +
        `<link rel="stylesheet" href="/assets/index-AAAAAAAA.css">`,
    );
    assert.throws(() => buildManifest(dist), /index-AAAAAAAA\.css/);
  });
});

test("a file the generator cannot name is a build failure, not a guess", () => {
  withDist({ "assets/notes.txt": "hello" }, (dist) => {
    assert.throws(() => buildManifest(dist), /notes\.txt/);
  });
});

// ───────────────────────────────────────────────────────────── the headers file

/** `_headers`: a pattern line, then indented `Name: value` lines; `#` is a comment. */
function parseHeaders(text) {
  const rules = new Map();
  let pattern = null;
  for (const line of text.split(/\r?\n/)) {
    if (line.trim() === "" || line.trimStart().startsWith("#")) continue;
    if (!/^\s/.test(line)) {
      pattern = line.trim();
      rules.set(pattern, rules.get(pattern) ?? { headers: {}, comments: [] });
      continue;
    }
    const match = line.match(/^\s+([A-Za-z-]+):\s*(.*)$/);
    assert.ok(match, `unparseable header line: ${line}`);
    assert.ok(pattern, `header line before any pattern: ${line}`);
    rules.get(pattern).headers[match[1].toLowerCase()] = match[2].trim();
  }
  return rules;
}

const headersText = () => {
  const path = join(WEB, "public", "_headers");
  assert.ok(existsSync(path), `${path} is missing: the deploy has no headers file`);
  return read(path);
};

/** Every rule whose pattern glob matches `path` — `*` spans segments in `_headers`. */
function matching(rules, path) {
  return [...rules.entries()].filter(([pattern]) => {
    const escaped = pattern.replace(/[.+?^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*");
    return new RegExp(`^${escaped}$`).test(path);
  });
}

test("the headers send application/wasm for the wasm", () => {
  const rules = parseHeaders(headersText());
  const wasm = matching(rules, "/assets/engine-BO4_EbTC.wasm");
  const types = wasm.map(([, rule]) => rule.headers["content-type"]).filter(Boolean);
  assert.deepEqual(types, ["application/wasm"]);
});

test("hashed assets cache forever, and only one rule sets Cache-Control on them", () => {
  const rules = parseHeaders(headersText());
  for (const path of [
    "/assets/engine-BO4_EbTC.wasm",
    "/assets/index-CvTX_Bx-.js",
    "/assets/index-BgWENkHr.css",
    "/assets/solve.worker-DZSqr1HT.js",
  ]) {
    const controls = matching(rules, path)
      .map(([, rule]) => rule.headers["cache-control"])
      .filter(Boolean);
    // Two matching rules would be comma-joined by the platform, turning
    // `immutable` into `immutable, public, max-age=0, must-revalidate`.
    assert.equal(controls.length, 1, `${path} is matched by ${controls.length} Cache-Control rules`);
    assert.equal(controls[0], "public, max-age=31536000, immutable");
  }
});

test("the page itself always revalidates", () => {
  const rules = parseHeaders(headersText());
  for (const path of ["/", "/index.html"]) {
    const controls = matching(rules, path)
      .map(([, rule]) => rule.headers["cache-control"])
      .filter(Boolean);
    assert.equal(controls.length, 1, `${path} is matched by ${controls.length} Cache-Control rules`);
    assert.match(controls[0], /max-age=0/);
    assert.match(controls[0], /must-revalidate/);
  }
});

test("every other response is hardened, and COOP/COEP stay a commented opt-in", () => {
  const text = headersText();
  const rules = parseHeaders(text);
  const fallback = matching(rules, "/manifest.json");
  assert.equal(fallback[0][1].headers["x-content-type-options"], "nosniff");
  // v1 runs one wasm instance per worker and no SharedArrayBuffer (§7), so both
  // headers must be absent from every live rule and present as a comment.
  for (const [, rule] of rules) {
    assert.equal(rule.headers["cross-origin-opener-policy"], undefined);
    assert.equal(rule.headers["cross-origin-embedder-policy"], undefined);
  }
  assert.match(text, /^#.*Cross-Origin-Opener-Policy: same-origin/m);
  assert.match(text, /^#.*Cross-Origin-Embedder-Policy: require-corp/m);
});

// ───────────────────────────────────────────────────────────── the deploy config

/** `wrangler.jsonc`: comments and trailing commas are legal, JSON.parse is not. */
function stripJsonc(text) {
  const out = [];
  for (const line of text.split(/\r?\n/)) {
    // Every `//` in this file is a comment: the config holds no URL.
    const cut = line.indexOf("//");
    out.push(cut === -1 ? line : line.slice(0, cut));
  }
  return out
    .join("\n")
    .replace(/,(\s*[}\]])/g, "$1")
    .trim();
}

const wranglerConfig = () => {
  const path = join(WEB, "wrangler.jsonc");
  assert.ok(existsSync(path), `${path} is missing: there is nothing to deploy with`);
  return JSON.parse(stripJsonc(read(path)));
};

test("the Worker is assets-only, so `_headers` applies and static requests stay free", () => {
  const config = wranglerConfig();
  assert.equal(config.name, "tetris-lab");
  assert.equal(config.main, undefined, "a `main` would make this a script deploy");
  assert.equal(config.build, undefined, "a `build` key would be a Cloudflare-side build");
  assert.equal(config.assets.directory, "./dist");
  assert.equal(config.assets.not_found_handling, "single-page-application");
  assert.match(config.compatibility_date, /^\d{4}-\d{2}-\d{2}$/);
});

test("workers_dev and preview URLs are written out, because a route turns both off", () => {
  const config = wranglerConfig();
  assert.equal(config.workers_dev, true);
  assert.equal(config.preview_urls, true);
  assert.equal(Array.isArray(config.routes), true);
  assert.deepEqual(config.routes, [{ pattern: "lab.samibentaleb.com", custom_domain: true }]);
});

// ──────────────────────────────────────────────────────────── the deploy workflow

const workflow = (name) =>
  readIf(join(ROOT, ".github", "workflows", name)) ??
  (() => assert.fail(`.github/workflows/${name} is missing`))();

/** Drop YAML comments, so a comment about rollback is not read as a rollback step. */
function commands(text) {
  return text
    .split(/\r?\n/)
    .map((line) => line.replace(/(^|\s)#.*$/, ""))
    .join("\n");
}

const deployText = () => {
  const raw = workflow("deploy.yml");
  return { raw, code: commands(raw) };
};

test("only a green gates run on main deploys", () => {
  const { code } = deployText();
  assert.match(code, /^on:/m);
  assert.match(code, /workflow_run:/);
  assert.match(code, /workflows:\s*\[gates\]/);
  assert.match(code, /types:\s*\[completed\]/);
  assert.match(code, /branches:\s*\[main\]/);
  assert.match(code, /github\.event\.workflow_run\.conclusion == 'success'/);
  // A `workflow_run` job runs on the default branch's ref, so the checkout has to
  // name the sha that was gated or it deploys whatever `main` happens to be.
  assert.match(code, /github\.event\.workflow_run\.head_sha/);
});

test("a hand-triggered preview uploads a version and deploys nothing", () => {
  const { code } = deployText();
  assert.match(code, /workflow_dispatch:/);
  assert.match(code, /wrangler versions upload/);
  assert.match(code, /--preview-alias/);
});

test("the deploy uploads the gated tree as one version, then smokes the live URL", () => {
  const { code } = deployText();
  assert.match(code, /actions\/download-artifact/);
  assert.match(code, /name:\s*dist/);
  assert.match(code, /npx wrangler deploy/);
  assert.match(code, /bash scripts\/verify\.sh --url/);
  assert.match(code, /node scripts\/browser-check\.mjs --url/);
  // The smoke steps come after the deploy: a smoke test that ran first would test
  // the previous version.
  assert.ok(
    code.indexOf("npx wrangler deploy") < code.indexOf("scripts/verify.sh"),
    "the smoke test runs before the deploy",
  );
});

test("two repository secrets, no rollback, and nothing that swallows a failure", () => {
  const { code, raw } = deployText();
  const secrets = [...code.matchAll(/secrets\.([A-Za-z0-9_]+)/g)].map((match) => match[1]);
  assert.deepEqual([...new Set(secrets)].sort(), ["CLOUDFLARE_ACCOUNT_ID", "CLOUDFLARE_API_TOKEN"]);
  assert.doesNotMatch(code, /continue-on-error/);
  assert.doesNotMatch(code, /wrangler rollback/);
  assert.doesNotMatch(code, /wrangler login/);
  assert.doesNotMatch(code, /wrangler pages/);
  // §10: "never auto-roll-back" — the file says so, so the next editor knows why
  // there is no handler here.
  assert.match(raw, /never (auto-)?roll/i);
});

test("the gates workflow hands its dist to the deploy, guarded to main", () => {
  const text = workflow("gates.yml");
  assert.match(text, /actions\/upload-artifact/);
  assert.match(text, /name:\s*dist\s*$/m);
  assert.match(text, /path:\s*web\/dist\s*$/m);
  assert.match(commands(text), /github\.ref == 'refs\/heads\/main'/);
});

// ─────────────────────────────────────────────────────────── the smoke scripts

const smoke = () => {
  const path = join(WEB, "scripts", "verify.sh");
  assert.ok(existsSync(path), `${path} is missing: there is no post-deploy smoke test`);
  const text = read(path);
  assert.match(text, /^#!.*\bbash\b/);
  assert.match(text, /--url/);
  return text;
};

test("verify.sh asserts headers, MIME, caching and the fallback", () => {
  const text = smoke();
  for (const needed of [
    "application/wasm", // the wasm MIME
    "text/html", // the page MIME
    "javascript", // the worker chunk is served as a script
    "application/json", // the manifest
    "public, max-age=31536000, immutable", // the exact hashed-asset value
    "max-age=0", // the page's value, pinned
    "must-revalidate",
    "etag",
    "nosniff",
    "this-path-does-not-exist", // the SPA fallback
    "manifest.json",
  ]) {
    assert.ok(text.includes(needed), `verify.sh never asserts ${needed}`);
  }
});

test("verify.sh handshakes the deployed wasm, and never rolls anything back", () => {
  const text = smoke();
  // The one assertion that names a stale artifact precisely: the live module's own
  // `abi_version` and the exports the loader needs.
  assert.ok(text.includes("abi_version"), "verify.sh never handshakes the live ABI");
  assert.ok(text.includes("gen_game"), "verify.sh never names the exports it needs");
  assert.doesNotMatch(commands(text), /rollback/);
  // Exit codes: 0 pass, 1 a mismatch, 2 usage or a missing prerequisite — the
  // convention host-probe/verify.sh established and ticket 03's gates follow.
  assert.match(text, /exit 2/);
  assert.match(text, /exit 1/);
});

test("browser-check.mjs proves the ABI handshake from a real browser", () => {
  const text = read(join(WEB, "scripts", "browser-check.mjs"));
  assert.ok(text.includes("ABI handshake agrees"), "browser-check.mjs does not assert the ABI handshake");
  assert.ok(text.includes("the page is the requested origin"), "browser-check.mjs does not assert the origin");
  assert.ok(text.includes('option("url"'), "browser-check.mjs cannot be pointed at a --url");
});

// ─────────────────────────────────────────────── the hashing rules, at the source

test("the page ships no service worker and no cache-busting query string", () => {
  for (const rel of ["index.html", "src/main.ts", "src/core/loader.ts"]) {
    const text = read(join(WEB, rel));
    assert.doesNotMatch(text, /serviceWorker/, `${rel} registers a service worker`);
    assert.doesNotMatch(text, /[?&]v=/, `${rel} hand-maintains a version query`);
  }
});

test("the build writes the manifest, and wrangler is a build-time tool", () => {
  const pkg = JSON.parse(read(join(WEB, "package.json")));
  assert.match(pkg.scripts.build, /manifest\.mjs/, "`npm run build` does not write dist/manifest.json");
  assert.ok(pkg.devDependencies.wrangler, "wrangler is not a devDependency");
  assert.equal(pkg.dependencies.wrangler, undefined, "wrangler must not be a runtime dependency");
  assert.deepEqual(pkg.dependencies, {}, "the page stays dependency-free");
});

test("the built page references its hashed assets, when a build is present", (t) => {
  const index = join(DIST, "index.html");
  if (!existsSync(index)) {
    t.skip("no dist/ yet — `npm run build` writes it, and `npm run gates` asserts it");
    return;
  }
  const html = read(index);
  const refs = [...html.matchAll(/(?:src|href)="(\/[^"]+)"/g)].map((match) => match[1]);
  assert.ok(refs.length > 0, "index.html references no asset at all");
  for (const ref of refs) {
    assert.match(ref, /^\/assets\/.*-[A-Za-z0-9_-]{8}\.[a-z0-9]+$/);
    assert.ok(existsSync(join(DIST, ref)), `index.html names ${ref}, which is not in dist/`);
  }
  const manifest = JSON.parse(read(join(DIST, "manifest.json")));
  const paths = Object.values(manifest.assets).map((asset) => asset.path);
  for (const ref of refs) assert.ok(paths.includes(ref), `${ref} is not in the manifest`);
  assert.doesNotMatch(html, /[?&]v=/);
});

// ────────────────────────────────────────────────────── the operator's two lines

test("the README names the two secrets and the one rollback command", () => {
  // §10: written down rather than executed. Whoever needs to revert a bad deploy
  // at 02:00 should not have to read a workflow to find the command.
  const readme = read(join(ROOT, "README.md"));
  for (const needed of [
    "CLOUDFLARE_API_TOKEN",
    "CLOUDFLARE_ACCOUNT_ID",
    "Workers Scripts",
    "npx wrangler rollback",
    "workflow_dispatch",
  ]) {
    assert.ok(readme.includes(needed), `README.md never mentions ${needed}`);
  }
});
