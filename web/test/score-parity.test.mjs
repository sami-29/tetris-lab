// Seam: native/wasm score parity — the test that makes "the browser number is not
// a guess" true (§5.4, ticket 05).
//
// The ABI carries no score. §4's exports return a packed result and a Move list,
// and the trace that does carry scores is `solve_traced` (ticket 11), so adding one
// here would mean a seventh export for a number the loader is not allowed to read.
// Parity is therefore proven through `solve`: §4.2's packed result and every word
// of the Move list, against the golden `cargo test` produced from the same
// Board-and-Sequence set. The Move list is the beam's layer-by-layer ranking played
// out, and the set includes Beam width 1 — a greedy arg-max — so a score that
// differs by one unit anywhere on the winning path shows up as a different move.
//
// The two suites read the same file. `engine/tests/fixtures/parity-report.txt` holds
// a `shipped` line naming the weight table the engine ships, and both sides compare
// only those rows: §9.4's verdict regenerates the report, which moves both or
// neither.
//
//   npm test        # builds web/pkg/engine.wasm first, then this

import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { ABI_VERSION, loadEngineFromBytes } from "../src/core/loader.ts";

const ARTIFACT = fileURLToPath(new URL("../pkg/engine.wasm", import.meta.url));
const REPORT = fileURLToPath(
  new URL("../../engine/tests/fixtures/parity-report.txt", import.meta.url),
);

function wasmBytes() {
  if (!existsSync(ARTIFACT)) {
    throw new Error(`missing ${ARTIFACT} — run \`node scripts/build-wasm.mjs\` first`);
  }
  return new Uint8Array(readFileSync(ARTIFACT));
}

/** §4.2's bit layout, written out again on purpose: the native side writes the
 * packed `i64` as a decimal and this decodes it independently of the loader, so a
 * field the loader gets wrong is a failure here rather than a match against itself. */
function unpackNative(packed) {
  return {
    won: (packed & 1n) === 1n,
    evaluations: Number((packed >> 1n) & 0xffff_ffffn),
    status: Number((packed >> 33n) & 0xffn),
    lines: Number((packed >> 41n) & 0xffn),
    moveCount: Number((packed >> 49n) & 0x1ffn),
  };
}

function parseReport() {
  const text = readFileSync(REPORT, "utf8");
  let shipped = null;
  const rows = [];
  for (const raw of text.split("\n")) {
    const line = raw.trim();
    if (line === "" || line.startsWith("#")) continue;
    if (line.startsWith("shipped ")) {
      shipped = line.slice("shipped ".length).trim();
      continue;
    }
    const f = line.split(/\s+/);
    assert.equal(f.length, 8, `not a parity row: ${line}`);
    rows.push({
      seed: Number(f[0]),
      pieces: Number(f[1]),
      cap: Number(f[2]),
      goal: Number(f[3]),
      beam: Number(f[4]),
      table: f[5],
      packed: BigInt(f[6]),
      moves: f[7] === "-" ? [] : f[7].split(",").map((word) => parseInt(word, 16) >>> 0),
    });
  }
  return { shipped, rows };
}

const { shipped, rows } = parseReport();
const shippedRows = rows.filter((row) => row.table === shipped);

test("the report names the shipped weight table and has rows to compare", () => {
  assert.ok(shipped !== null, "parity-report.txt has no `shipped` line");
  assert.ok(["e0", "e1"].includes(shipped), `unknown table \`${shipped}\``);
  assert.ok(
    shippedRows.length >= 4,
    `only ${shippedRows.length} rows for \`${shipped}\` — the parity set shrank`,
  );
  // The wasm can only reach `shipped()`, so a row it cannot run would be dead data.
  assert.equal(rows.length, shippedRows.length * 2, "the report holds both tables");
});

test("every case in the set solves to the same result through the built wasm", async () => {
  const engine = await loadEngineFromBytes(wasmBytes());
  assert.equal(engine.abiVersion, ABI_VERSION);

  for (const row of shippedRows) {
    const where = `seed ${row.seed} pieces ${row.pieces} cap ${row.cap} goal ${row.goal} beam ${row.beam}`;

    // The Board and Sequence are not in the fixture: both targets generate them
    // from the same Seed with the same committed generator (§5.5–5.8).
    const generated = engine.genGame(row.seed, row.pieces, row.cap);
    assert.equal(generated, 0, `${where}: gen_game refused an in-domain case`);

    const outcome = engine.solve(row.goal, row.beam);
    const native = unpackNative(row.packed);
    assert.equal(outcome.status, native.status, `${where}: status`);
    assert.equal(outcome.won, native.won, `${where}: won`);
    assert.equal(outcome.evaluations, native.evaluations, `${where}: evaluations`);
    assert.equal(outcome.lines, native.lines, `${where}: lines`);
    assert.equal(outcome.moveCount, native.moveCount, `${where}: move count`);

    const moves = Uint32Array.from(engine.moves(outcome.moveCount));
    assert.deepEqual(
      [...moves],
      row.moves,
      `${where}: the Move list differs from the native one`,
    );
  }
});

test("the compared rows are score-sensitive, not just legal-move parity", async () => {
  // Two claims worth failing on: the set exercises a real Move list (so it is
  // comparing rankings, not two empty answers), and it exercises "no answer" too.
  const winning = shippedRows.filter((row) => row.moves.length > 0);
  const silent = shippedRows.filter((row) => row.moves.length === 0);
  assert.ok(winning.length >= 3, `only ${winning.length} cases return a Move list`);
  assert.ok(silent.length >= 1, "no case in the set exercises a lost Solve");

  const engine = await loadEngineFromBytes(wasmBytes());
  // A game lost by both tables proves the two targets agree about an absence as
  // well as about a number; the packed result still carries evaluations and Lines.
  const lost = silent[0];
  assert.equal(engine.genGame(lost.seed, lost.pieces, lost.cap), 0);
  const outcome = engine.solve(lost.goal, lost.beam);
  assert.equal(outcome.won, false);
  assert.equal(outcome.moveCount, 0);
  assert.ok(outcome.evaluations > 0, "a lost Solve still evaluated placements");
});
