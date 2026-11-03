// Seam 2: the real wasm artifact through the real loader.
//
//   npm test    # = node scripts/build-wasm.mjs && node --test "test/**/*.test.mjs"
//
// Everything the Playground promises about *changing* the game is a property of the
// generator, and the generator only exists in wasm. Three of them are worth a real
// test rather than a comment, because each is a claim the page makes in words:
//
//   * **raising the starting height keeps the Sequence and changes the Board**
//     (ticket bullet 3, §5.7). The proof is an arity fact, not a coincidence:
//     `gen_game(seed, pieces, height_cap)` takes no goal and `solve(goal, beam)`
//     takes no seed, so two caps with one seed can only differ in the Board.
//     Measured before writing this test: 4 seeds, 0 sequence mismatches, 4
//     different Boards.
//   * **a game the search cannot win is an outcome, not an error** (ticket bullet
//     8): `status 0`, `won false`, no Move list, and the Lines it reached — which
//     is the number the page has to say out loud. Measured: goal 20 on 40 pieces at
//     the default width loses with 16 Lines cleared and 0 Moves.
//   * **the two presets are the committed games** (ticket bullet 2): seed 0 for
//     each configuration must generate the Board and Sequence that
//     `data/dataset/<id>/games.txt` commits, and win its Goal at `DEFAULT_BEAM`.
//     That is what "reproduce the figures quoted for them" can mean without
//     re-running §9.6's 2,000 solves.

import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { loadEngineFromBytes } from "../src/core/loader.ts";
import { DEFAULT_BEAM, WIDTH_CAP } from "../src/config/width.ts";
import { PRESETS, presetGame } from "../src/ui/presets.ts";

const ARTIFACT = fileURLToPath(new URL("../pkg/engine.wasm", import.meta.url));
const DATASET = fileURLToPath(new URL("../../data/dataset/", import.meta.url));

function wasmBytes() {
  if (!existsSync(ARTIFACT)) {
    throw new Error(`missing ${ARTIFACT} — run \`node scripts/build-wasm.mjs\` first`);
  }
  return new Uint8Array(readFileSync(ARTIFACT));
}

const engine = await loadEngineFromBytes(wasmBytes());

/** One generated game, as the worker reads it out of wasm memory. */
function generate(seed, pieces, heightCap) {
  const status = engine.genGame(seed, pieces, heightCap);
  assert.equal(status, 0, `gen_game(${seed}, ${pieces}, ${heightCap})`);
  return { board: Array.from(engine.board()), sequence: engine.sequence(pieces) };
}

// ── one control, one thing ─────────────────────────────────────────────────

test("raising the starting height keeps the Sequence and changes only the Board", () => {
  for (const seed of [0, 1, 42, 12345]) {
    const low = generate(seed, 40, 4);
    const high = generate(seed, 40, 19);
    assert.equal(
      high.sequence,
      low.sequence,
      `seed ${seed}: the Sequence must not depend on the starting height (§5.7)`,
    );
    assert.notDeepEqual(
      high.board,
      low.board,
      `seed ${seed}: the Board must depend on the starting height (§5.7)`,
    );
  }
});

test("a longer game nests the Sequence and leaves the Board alone", () => {
  // §5.7: "a 100-piece sequence begins with the 40-piece sequence for the same
  // seed" — so two shipped configurations pair on a seed as the same Board.
  const p40 = generate(0, 40, 7);
  const p100 = generate(0, 100, 7);
  const p200 = generate(0, 200, 7);
  assert.equal(p100.sequence.slice(0, 40), p40.sequence);
  assert.equal(p200.sequence.slice(0, 40), p40.sequence);
  assert.deepEqual(p100.board, p40.board);
  assert.deepEqual(p200.board, p40.board);
});

// ── the clamp's output is always a game the contract accepts ───────────────

test("every corner of the clamp is a game the engine names rather than refuses", () => {
  // "The UI must not offer a value the contract refuses to name" (§2.2). The
  // corners are the values the clamp can produce at its extremes.
  const corners = [
    [0, 1, 2],
    [0, 200, 19],
    [4294967295, 1, 19],
    [4294967295, 200, 2],
    [0, 40, 7],
  ];
  for (const [seed, pieces, heightCap] of corners) {
    assert.equal(engine.genGame(seed, pieces, heightCap), 0, `gen_game(${seed}, ${pieces}, ${heightCap})`);
    const outcome = engine.solve(1, 1);
    assert.notEqual(outcome.status, 1, "goal 1 / beam 1 is in the Domain");
    const top = engine.solve(255, 1);
    assert.notEqual(top.status, 1, "goal 255 is in the Domain (the packed Lines field is 8 bits)");
  }
  // The width cap is the UI's bound, not the ABI's: the engine must accept it even
  // though it refuses to *offer* anything wider (§2.2, §5.8).
  assert.equal(engine.genGame(0, 40, 7), 0);
  assert.notEqual(engine.solve(8, WIDTH_CAP).status, 1, `the cap width ${WIDTH_CAP} is in the Domain`);
});

// ── a loss is an outcome ───────────────────────────────────────────────────

test("a game the search cannot win reports the Lines it cleared and no Moves", () => {
  generate(0, 40, 7);
  const outcome = engine.solve(20, DEFAULT_BEAM);
  assert.equal(outcome.status, 0, "a loss is not a failure code (§4.2 cuts the other way)");
  assert.equal(outcome.won, false);
  assert.ok(outcome.lines > 0, `it reached ${outcome.lines} Lines, which is what the page says`);
  assert.ok(outcome.lines < 20, "and it did not reach the Goal");
  assert.equal(outcome.moveCount, 0, "the loss path returns no Move list to replay");
  assert.equal(Array.from(engine.moves(outcome.moveCount)).length, 0);
});

// ── the two shipped configurations ─────────────────────────────────────────

test("each preset is the committed game, and it wins its Goal at the default width", () => {
  for (const preset of PRESETS) {
    const game = presetGame(preset, { defaultBeam: DEFAULT_BEAM, widthCap: WIDTH_CAP });
    const generated = generate(game.seed, game.pieces, game.heightCap);

    // The Dataset is the oracle: `games.txt`'s seed 0 row is the game this button
    // claims to be.
    const line = readFileSync(`${DATASET}${preset.id}/games.txt`, "utf8")
      .split("\n")
      .find((row) => row.startsWith(`${game.seed} `));
    assert.ok(line, `${preset.id}: games.txt has seed ${game.seed}`);
    const [, ...words] = line.trim().split(" ");
    const committedBoard = words.slice(0, 10).map((word) => Number.parseInt(word, 16) >>> 0);
    const committedSequence = words[10];
    assert.deepEqual(generated.board, committedBoard, `${preset.id}: the committed Board`);
    assert.equal(generated.sequence, committedSequence, `${preset.id}: the committed Sequence`);

    const outcome = engine.solve(game.goal, game.beamWidth);
    assert.equal(outcome.status, 0);
    assert.equal(outcome.won, true, `${preset.id} wins at the default width`);
    assert.ok(outcome.lines >= game.goal, `${preset.id}: ${outcome.lines} Lines cleared`);
    assert.ok(outcome.moveCount > 0, "a win has a Move list to replay");
  }
});
