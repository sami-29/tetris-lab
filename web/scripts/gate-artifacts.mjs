#!/usr/bin/env node
// The two things gates 4 and 6 need from Node: a wasm instance, and gzip.
//
//   node scripts/gate-artifacts.mjs parity --seeds 0-99 --beam 25 \
//       --config goal8-p40-cap7:8:40:7 --config goal12-p100-cap7:12:100:7
//   node scripts/gate-artifacts.mjs sizes
//
// `parity` loads the *real* artifact (`pkg/engine.wasm`) through the *real*
// loader (`src/core/loader.ts` — the one file in the app that mentions
// `WebAssembly`) and prints one line per (configuration, seed):
//
//   line config=goal8-p40-cap7 seed=0 won=1 status=0 evaluations=10212 lines=8 moves=20
//
// `bench gate 4` computes the same rows natively and compares them field by
// field, so a mismatch names the seed, the field and both values. The protocol is
// line-oriented `key=value` rather than JSON: the bench crate has no serde, and a
// hand-rolled JSON reader in Rust would be a liability in exchange for nothing.
//
// `sizes` walks `dist/`, gzips every html/js/css/wasm asset in memory, and prints
// the per-file sizes plus one total. §9.3's whole-page budget is asserted without a
// server and without a network: `gzipSync` at Node's default level, on the bytes
// that are actually uploaded.
//
// Both modes resolve their paths from `import.meta.url`, never from the cwd, so
// `bench gate` can spawn this from any directory (the rule
// `scripts/browser-check.mjs` already follows). Exit codes: 0 measured, 1 could
// not measure, 2 usage.

import { readFileSync, readdirSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, relative, sep } from "node:path";
import { gzipSync } from "node:zlib";

import { loadEngineFromBytes } from "../src/core/loader.ts";

const HERE = dirname(fileURLToPath(import.meta.url));
const WEB = join(HERE, "..");
const ARTIFACT = join(WEB, "pkg", "engine.wasm");
const DIST = join(WEB, "dist");

/** §9.3's page budget counts these. Source maps are excluded: they are not the payload. */
const COUNTED = new Set([".html", ".js", ".css", ".wasm"]);

function usage(message) {
  if (message) console.error(`error: ${message}`);
  console.error(
    [
      "usage:",
      "  node scripts/gate-artifacts.mjs parity --seeds <first>-<last> --beam <n> \\",
      "      --config <name>:<goal>:<pieces>:<cap> [--config ...]",
      "  node scripts/gate-artifacts.mjs sizes",
    ].join("\n"),
  );
  process.exit(2);
}

function option(argv, name) {
  const index = argv.indexOf(`--${name}`);
  return index === -1 ? undefined : argv[index + 1];
}

function options(argv, name) {
  const values = [];
  for (let index = 0; index < argv.length; index += 1) {
    if (argv[index] === `--${name}` && argv[index + 1] !== undefined) {
      values.push(argv[index + 1]);
    }
  }
  return values;
}

function whole(text, what) {
  const value = Number(text);
  if (!Number.isInteger(value) || value < 0) usage(`${what} must be a whole number, found ${text}`);
  return value;
}

// ─────────────────────────────────────────────────────────────── gate 4: parity

async function parity(argv) {
  const seeds = option(argv, "seeds");
  if (seeds === undefined) usage("parity needs --seeds <first>-<last>");
  const [first, last] = seeds.split("-").map((part) => whole(part, "--seeds"));
  if (last < first) usage(`--seeds range ${seeds} ends before it starts`);
  const beam = whole(option(argv, "beam") ?? "25", "--beam");

  const configs = options(argv, "config").map((spec) => {
    const [name, goal, pieces, cap] = spec.split(":");
    if (!name || goal === undefined || pieces === undefined || cap === undefined) {
      usage(`--config must be <name>:<goal>:<pieces>:<cap>, found ${spec}`);
    }
    return {
      name,
      goal: whole(goal, `${name} goal`),
      pieces: whole(pieces, `${name} pieces`),
      cap: whole(cap, `${name} cap`),
    };
  });
  if (configs.length === 0) usage("parity needs at least one --config");

  let bytes;
  try {
    bytes = new Uint8Array(readFileSync(ARTIFACT));
  } catch (error) {
    console.error(`error: cannot read ${ARTIFACT}: ${error.message}`);
    console.error("hint: npm --prefix web run build:wasm");
    process.exit(1);
  }

  const engine = await loadEngineFromBytes(bytes);
  const lines = [];
  for (const config of configs) {
    for (let seed = first; seed <= last; seed += 1) {
      const generated = engine.genGame(seed, config.pieces, config.cap);
      if (generated !== 0) {
        // A Domain failure is a value, not a trap (§4.2): report it as a status.
        lines.push(
          `line config=${config.name} seed=${seed} won=0 status=${generated} evaluations=0 lines=0 moves=0`,
        );
        continue;
      }
      const result = engine.solve(config.goal, beam);
      lines.push(
        `line config=${config.name} seed=${seed} won=${result.won ? 1 : 0} ` +
          `status=${result.status} evaluations=${result.evaluations} ` +
          `lines=${result.lines} moves=${result.moveCount}`,
      );
    }
  }
  process.stdout.write(`${lines.join("\n")}\n`);
  process.exit(0);
}

// ─────────────────────────────────────────────────────────────── gate 6: sizes

function walk(dir) {
  const out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walk(path));
    else out.push(path);
  }
  return out;
}

function sizes() {
  let files;
  try {
    if (!statSync(DIST).isDirectory()) throw new Error("not a directory");
    files = walk(DIST);
  } catch (error) {
    console.error(`error: cannot read ${DIST}: ${error.message}`);
    console.error("hint: npm --prefix web run build");
    process.exit(1);
  }

  const measured = [];
  for (const path of files.sort()) {
    const dot = path.lastIndexOf(".");
    const extension = dot === -1 ? "" : path.slice(dot);
    if (!COUNTED.has(extension)) continue;
    const raw = readFileSync(path);
    measured.push({
      name: relative(DIST, path).split(sep).join("/"),
      raw: raw.length,
      gzip: gzipSync(raw).length,
    });
  }
  if (measured.length === 0) {
    console.error(`error: ${DIST} holds no html/js/css/wasm asset`);
    console.error("hint: npm --prefix web run build");
    process.exit(1);
  }

  const total = measured.reduce((sum, file) => sum + file.gzip, 0);
  const rawTotal = measured.reduce((sum, file) => sum + file.raw, 0);
  const lines = measured.map((file) => `file name=${file.name} raw=${file.raw} gzip=${file.gzip}`);
  lines.push(`size total=${total} raw=${rawTotal} files=${measured.length}`);
  process.stdout.write(`${lines.join("\n")}\n`);
  process.exit(0);
}

// ───────────────────────────────────────────────────────────────────── dispatch

const argv = process.argv.slice(2);
switch (argv[0]) {
  case "parity":
    await parity(argv.slice(1));
    break;
  case "sizes":
    sizes();
    break;
  default:
    usage(argv[0] === undefined ? "no mode given" : `unknown mode ${argv[0]}`);
}
