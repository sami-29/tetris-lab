#!/usr/bin/env node
// The Explainer's facts, generated from the committed artifacts.
//
// Ticket 16's last bullet: "Numbers that later tickets can move (DEFAULT_BEAM, the
// cap, the wasm size, the shipped table) must be read from the artifacts, not typed
// into prose, so the drawer cannot go stale." Three of those already have a home the
// page imports directly — `src/config/width.ts` reads
// `data/measurements/width-curve.json`, and `src/config/evaluator.ts` carries the
// shipped table under a test that re-parses `engine/src/evaluator.rs`. What is left
// is the set of facts no browser module can read for itself:
//
//   * `TAIL` — a Rust constant inside the search;
//   * the Dataset's shape — the committed manifests, enumerated rather than named;
//   * the ablation ladder's verdict — which table ships and which switches the
//     ladder deleted;
//   * the Reference run's labels — `data/dataset/<config>/reference.jsonl`, which
//     ticket 09 appends to. While no row is committed the fact says so, and the
//     Explainer quotes no machine.
//
//   node scripts/build-facts.mjs        # writes src/generated/engine-facts.ts
//
// It is idempotent, it runs before `tsc` and before `node --test`, and it never
// invents a value: a missing artifact is an error naming the path, because a figure
// with no source is exactly what this ticket forbids.
//
// Nothing here reads the clock or the working directory: the file is a function of
// the committed artifacts, so its diff is a measurement's diff and nothing else.

import { mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";

const HERE = fileURLToPath(new URL(".", import.meta.url));
const WEB = join(HERE, "..");
const ROOT = join(WEB, "..");
const OUT = join(WEB, "src", "generated", "engine-facts.ts");
mkdirSync(join(WEB, "src", "generated"), { recursive: true });

const read = (...parts) => readFileSync(join(ROOT, ...parts), "utf8");
const json = (...parts) => JSON.parse(read(...parts));

// ── TAIL: the engine's own constant ────────────────────────────────────────

function tailFromEngine() {
  const rust = read("engine", "src", "search.rs");
  const match = rust.match(/pub const TAIL: usize = (\d+);/);
  if (!match) throw new Error("engine/src/search.rs no longer declares `pub const TAIL: usize = N;`");
  return Number(match[1]);
}

// ── the ablation ladder's verdict ──────────────────────────────────────────

function shippedFromLadder() {
  const ladder = json("data", "measurements", "ablation-ladder.json");
  const shipped = ladder.shipped;
  if (!shipped) throw new Error("data/measurements/ablation-ladder.json has no `shipped` block");
  for (const key of ["evaluator", "table", "features", "toggles", "deleted_toggles", "decided_by"]) {
    if (shipped[key] === undefined) throw new Error(`ablation-ladder.json: shipped.${key} is missing`);
  }
  return {
    evaluator: shipped.evaluator,
    table: shipped.table,
    features: shipped.features,
    toggles: shipped.toggles,
    deletedToggles: shipped.deleted_toggles,
    decidedBy: shipped.decided_by,
    source: "data/measurements/ablation-ladder.json · shipped",
  };
}

// ── the Dataset: enumerated from the committed directories ────────────────

function datasetFromManifests() {
  const names = readdirSync(join(ROOT, "data", "dataset"), { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort();
  if (names.length === 0) throw new Error("data/dataset/ holds no configuration");

  const configs = names.map((name) => {
    const manifest = json("data", "dataset", name, "manifest.json");
    return {
      name,
      goal: manifest.goal,
      pieces: manifest.pieces,
      cap: manifest.cap,
      games: manifest.game_count,
      test: { first: manifest.splits.test.first, last: manifest.splits.test.last },
      tuning: { first: manifest.splits.tuning.first, last: manifest.splits.tuning.last },
      fingerprint: manifest.fingerprint,
    };
  });

  // The drawer prints one game count and one pair of seed ranges, so the manifests
  // must agree. A configuration that disagrees is a second Dataset, not a row.
  const first = configs[0];
  for (const config of configs) {
    if (config.games !== first.games) throw new Error(`${config.name}: ${config.games} games, ${first.name}: ${first.games}`);
    if (config.test.first !== first.test.first || config.test.last !== first.test.last) {
      throw new Error(`${config.name} and ${first.name} disagree about the test Split`);
    }
    if (config.tuning.first !== first.tuning.first || config.tuning.last !== first.tuning.last) {
      throw new Error(`${config.name} and ${first.name} disagree about the tuning Split`);
    }
  }

  const group = (value) => value.toLocaleString("en-US");
  return {
    configCount: configs.length,
    gameCount: first.games,
    games: group(first.games),
    test: first.test,
    tuning: first.tuning,
    testRange: `${group(first.test.first)}–${group(first.test.last)}`,
    tuneRange: `${group(first.tuning.first)}–${group(first.tuning.last)}`,
    configs,
  };
}

// ── the Reference run: the latest row per configuration, if any ───────────

/** A dotted lookup that tolerates the two shapes a label may take. */
function field(row, ...paths) {
  for (const path of paths) {
    let value = row;
    for (const key of path.split(".")) value = value?.[key];
    if (typeof value === "string" && value.length > 0) return value;
    if (value && typeof value === "object" && typeof value.label === "string") return value.label;
  }
  return null;
}

function referenceFromRows(names) {
  const rows = [];
  for (const name of names) {
    const text = read("data", "dataset", name, "reference.jsonl");
    const lines = text.split(/\r?\n/).filter((line) => line.trim());
    if (lines.length === 0) continue;
    const latest = JSON.parse(lines[lines.length - 1]); // append-only: the last entry is the latest
    const labels = {
      config: name,
      machine: field(latest, "machine", "cpu", "host", "labels.machine"),
      runtime: field(latest, "runtime", "node", "browser", "labels.runtime"),
      date: field(latest, "date", "measured_at", "generated_at", "labels.date"),
      commit: field(latest, "engine.commit", "engine_commit", "commit", "engine.hash", "labels.engine_commit"),
    };
    const missing = ["machine", "runtime", "date", "commit"].filter((key) => labels[key] === null);
    if (missing.length > 0) {
      throw new Error(
        `data/dataset/${name}/reference.jsonl: the latest row has no ${missing.join(", ")} label ` +
          `(keys seen: ${Object.keys(latest).join(", ")}). Ticket 09 owns this file's shape; ` +
          "the Explainer will not quote a machine it cannot name.",
      );
    }
    // Ticket 15's Run panel shows the Reference row's own statistics, not a copy typed
    // into the page: the latest row ships whole, in the file's own field names, so
    // `core/reference.ts` parses the generated object with the same code that parses a
    // line (one implementation of the field names, and no constant to go stale).
    //
    // One value is edited, and only its keyword: the page's glossary bans `target`
    // (Goal's avoided near-synonym), so the build label ships the architecture and
    // every flag — `wasm32-unknown-unknown`, lto, panic, strip — without the Rust word
    // in front of it. `run-stats.test.mjs` asserts that this is the only difference.
    const row = structuredClone(latest);
    if (row.engine && typeof row.engine.profile === "string") {
      row.engine.profile = row.engine.profile.replace(/\btarget\s+/gi, "");
    }
    rows.push({ ...labels, row });
  }
  return {
    status: rows.length === 0 ? "pending" : "measured",
    rows,
    source: "data/dataset/<configuration>/reference.jsonl · the latest row",
  };
}

// ── write the module ────────────────────────────────────────────────────

const tail = tailFromEngine();
const shipped = shippedFromLadder();
const dataset = datasetFromManifests();
const reference = referenceFromRows(dataset.configs.map((config) => config.name));

const body = `// Generated by \`web/scripts/build-facts.mjs\` — do not edit.
//
// Every value here is read from a committed artifact, named in \`source\`, and
// re-checked by \`web/test/explainer.test.mjs\` against its own parse of the same
// artifact. Regenerate with \`npm run build:facts\` (the build and the tests both do
// it first), so a measurement lands in the Explainer by re-running a command, never
// by editing prose.
//
// \`DEFAULT_BEAM\`, the Width cap and the shipped Weight table are **not** here:
// \`src/config/width.ts\` and \`src/config/evaluator.ts\` already own them, each under a
// test that re-reads the artifact it came from.

export interface DatasetSplitFact {
  readonly first: number;
  readonly last: number;
}

export interface DatasetConfigFact {
  readonly name: string;
  readonly goal: number;
  readonly pieces: number;
  readonly cap: number;
  readonly games: number;
  readonly test: DatasetSplitFact;
  readonly tuning: DatasetSplitFact;
  readonly fingerprint: string;
}

export interface ReferenceRowFact {
  readonly config: string;
  readonly machine: string;
  readonly runtime: string;
  readonly date: string;
  readonly commit: string;
  /** The latest row verbatim (its engine.profile without the Rust "target" keyword). */
  readonly row: Readonly<Record<string, unknown>>;
}

export interface EngineFacts {
  readonly tail: { readonly pieces: number; readonly source: string };
  readonly shipped: {
    readonly evaluator: string;
    readonly table: string;
    readonly features: number;
    readonly toggles: string;
    readonly deletedToggles: readonly string[];
    readonly decidedBy: string;
    readonly source: string;
  };
  readonly dataset: {
    readonly configCount: number;
    readonly gameCount: number;
    readonly games: string;
    readonly test: DatasetSplitFact;
    readonly tuning: DatasetSplitFact;
    readonly testRange: string;
    readonly tuneRange: string;
    readonly configs: readonly DatasetConfigFact[];
  };
  readonly reference: {
    readonly status: "pending" | "measured";
    readonly rows: readonly ReferenceRowFact[];
    readonly source: string;
  };
}

export const ENGINE_FACTS: EngineFacts = ${JSON.stringify(
  {
    tail: { pieces: tail, source: "engine/src/search.rs · pub const TAIL" },
    shipped,
    dataset,
    reference,
  },
  null,
  2,
)};
`;

writeFileSync(OUT, body);

console.log(
  `engine-facts.ts · TAIL ${tail} · ${shipped.evaluator} ${shipped.table} · ` +
    `${dataset.configCount} configurations × ${dataset.games} games · reference ${reference.status}`,
);
