// Seam 3: the bounded tree — top states per Layer, ticks for dead ends, the
// off-selection dimmed — built from the committed trace fixture with **no solver,
// no wasm, no worker, no DOM**.
//
// The trace is ticket 11's: `decodeTrace` turns §4.4's words into records and this
// file hands those records to `buildTree`. Nothing here re-derives a number the
// engine said. There is no scoring, no expansion and — the assertion at the bottom
// of this file — no sort: the top `bound` states of a Layer are its first `bound`
// records, because `rank` is the engine's own order and a JavaScript sort would be
// a second implementation of it that silently disagrees on ties.
//
// What this file asserts, in the ticket's own words:
//
//   * the node count at top 24, per Layer, and that `bound` above the band never
//     pads a column;
//   * a state with no child in the trace is a **tick**, and the last Layer (which
//     has no successor to drop it) is not;
//   * an edge whose parent is not drawn is a **stem** that says why — the `-2`
//     sentinel included, where the honest answer is "the trace did not record the
//     parent" and never a line to the column edge as though the parent were rank 0;
//   * selecting a state dims everything off its path, and the alphas are the
//     exported constants rather than numbers read off a canvas;
//   * the tree's bound and the beam width are two numbers: the bound rebuilds the
//     picture from the trace already in memory and calls nothing.
//
// The numbers below were recomputed from the committed fixture's words by an
// independent implementation (Python, `E:/Work/tetris-algo/.scratch/run-scratch12/`)
// before `buildTree` existed, and are also recorded in the fixture's `expect.tree`
// block so the artifact and the test carry the same claim.

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { decodeTrace } from "../src/ui/trace.ts";
import {
  TREE_ALPHA,
  TREE_BOUND_DEFAULT,
  TREE_BOUND_MAX,
  TREE_BOUND_MIN,
  TREE_BOUND_STEP,
  TREE_COPY,
  ancestryOf,
  buildTree,
  clampTreeBound,
  descentOf,
  pickState,
  stateCard,
  stateCardLayout,
  treeBoundLimit,
  treeDisplayList,
} from "../src/ui/tree-model.ts";

const FIXTURE = JSON.parse(
  readFileSync(
    fileURLToPath(new URL("./fixtures/trace-seed0-goal8-p40-cap7-band25.json", import.meta.url)),
    "utf8",
  ),
);

/** The trace the band reads, decoded by the same decoder the page runs. */
const TRACE = decodeTrace(FIXTURE.traceWords, FIXTURE.band);

/** The inspector's plot at 2560×1440: 590 CSS px wide, 786 tall (notes §3.1/§3.3). */
const PLOT = { width: 590, height: 786 };
const tree = (options = {}) => buildTree(TRACE, { ...PLOT, ...options });

const kinds = (model) => treeDisplayList(model).filter((mark) => mark.kind === "edge" || mark.kind === "state");
const ofKind = (model, kind) => treeDisplayList(model).filter((mark) => mark.kind === kind);
const ids = (marks) => marks.map((mark) => mark.id).sort();
const layerOf = (layer) => TRACE.records.slice(TRACE.layerStart[layer], TRACE.layerStart[layer] + TRACE.perLayerCounts[layer]);
const expectedDrawn = (bound) => FIXTURE.expect.perLayerCounts.map((count) => Math.min(bound, count));

/** A trace built by hand, for the cases the committed fixture cannot contain. */
function synthetic(layers) {
  const words = [0];
  for (const [layer, records] of layers.entries()) {
    for (const record of records) words.push(layer, record.rank, record.score, record.lines, record.parentRank);
  }
  words[0] = (words.length - 1) / 5;
  return decodeTrace(Int32Array.from(words), 25);
}

// ── the same trace the band reads ──────────────────────────────────────────

test("the tree reads the trace the band reads, not a second recording of it", () => {
  assert.equal(TRACE.records.length, FIXTURE.expect.records, "the fixture's record count moved");
  assert.equal(TRACE.layerCount, FIXTURE.expect.layers, "the fixture's layer count moved");
  const model = tree();
  assert.equal(model.records, TRACE.records.length, "the model counts the trace's own records");
  assert.equal(model.layerCount, TRACE.layerCount);
  assert.equal(model.band, FIXTURE.band, "the band the trace was recorded at travels with the model");
});

// ── the bound: 24 per Layer, and the fixture says how many ─────────────────

test("the tree draws at most 24 states per Layer, and the fixture says how many", () => {
  const model = tree();
  assert.equal(TREE_BOUND_DEFAULT, 24, "§2.3: the measured bound is top 24 states per layer");
  assert.equal(model.bound, TREE_BOUND_DEFAULT);
  assert.deepEqual(
    model.counts.perLayerDrawn,
    FIXTURE.expect.tree.perLayerDrawn,
    "per-layer drawn counts moved",
  );
  assert.deepEqual(model.counts.perLayerDrawn, expectedDrawn(24), "a column is min(bound, the Layer's records)");
  assert.equal(model.nodes.length, FIXTURE.expect.tree.nodes, "the node count moved");
  assert.equal(model.nodes.length, 427);
  assert.equal(
    model.counts.perLayerDrawn.reduce((sum, count) => sum + count, 0),
    model.nodes.length,
    "the columns add up to the node list",
  );
});

test("a bound above the band never pads a column: the band is the hard ceiling", () => {
  const atForty = tree({ bound: 40 });
  const atSixty = tree({ bound: 60 });
  assert.equal(atForty.nodes.length, FIXTURE.expect.tree.nodesAtBound["40"]);
  assert.equal(atSixty.nodes.length, FIXTURE.expect.tree.nodesAtBound["60"]);
  // The fixture is a band-25 trace: 20 Layers, 444 records. Asking for 40 or 60
  // cannot invent records, so both draw the whole band and neither stretches a
  // short Layer.
  assert.equal(atForty.nodes.length, TRACE.records.length);
  assert.equal(atSixty.nodes.length, TRACE.records.length);
  for (const layer of [0, 1, TRACE.layerCount - 1]) {
    assert.equal(
      atSixty.counts.perLayerDrawn[layer],
      FIXTURE.expect.perLayerCounts[layer],
      `layer ${layer} is ragged, never padded`,
    );
  }
  assert.equal(treeBoundLimit(FIXTURE.band), Math.min(TREE_BOUND_MAX, FIXTURE.band), "the control's ceiling is min(60, band)");
  assert.equal(clampTreeBound(60, 12), 12, "a band below the ceiling drops the ceiling with it");
  assert.equal(clampTreeBound(1, 25), TREE_BOUND_MIN, "the floor keeps a Layer from being a single dot");
  assert.equal(TREE_BOUND_MIN, 4);
  assert.equal(TREE_BOUND_STEP, 2);
  assert.equal(TREE_BOUND_MAX, 60, "the haze FINDINGS measured");
});

test("the bound is a slice of the trace, never a sort", () => {
  const model = tree();
  for (let layer = 0; layer < TRACE.layerCount; layer += 1) {
    const drawn = model.nodes.filter((node) => node.layer === layer);
    assert.deepEqual(
      drawn.map((node) => node.rank),
      Array.from({ length: model.counts.perLayerDrawn[layer] }, (_, rank) => rank),
      `layer ${layer} draws ranks 0..n-1 in the trace's own order`,
    );
    // Rank order *is* score order — the engine's ordering, not one we invented.
    const records = layerOf(layer);
    for (const node of drawn) assert.equal(node.score, records[node.rank].score);
  }
  assert.deepEqual(model.orderViolation, [], "the fixture is score-descending within every Layer");

  // A Layer that is not score-descending is reported, never repaired: the tree
  // must not grow a second ordering implementation.
  const unsorted = synthetic([
    [{ rank: 0, score: -100, lines: 0, parentRank: -1 }],
    [
      { rank: 0, score: -101, lines: 0, parentRank: 0 },
      { rank: 1, score: -99, lines: 0, parentRank: 0 },
    ],
  ]);
  const repaired = buildTree(unsorted, { ...PLOT, bound: 24 });
  assert.deepEqual(repaired.orderViolation, [1], "the model says which Layer is out of order");
  assert.deepEqual(
    repaired.nodes.filter((node) => node.layer === 1).map((node) => node.rank),
    [0, 1],
    "and still draws them in array order — no sort, no swap",
  );
});

// ── the tick: no child in the trace ───────────────────────────────────────

test("a state with no child in the trace is a tick, not an empty node", () => {
  const model = tree();
  const ticks = ofKind(model, "tick");
  assert.equal(ticks.length, FIXTURE.expect.tree.ticks, "the tick count moved");
  assert.equal(ticks.length, 187);
  assert.deepEqual(
    model.counts.perLayerTicks,
    FIXTURE.expect.tree.perLayerTicks,
    "per-layer tick counts moved",
  );
  for (const tick of ticks) {
    const node = model.nodes.find((candidate) => candidate.id === tick.id);
    assert.equal(node.kidsInTrace, 0, `${tick.id} has a child in the trace, so it is not a tick`);
    assert.equal(node.deadEnd, true);
    assert.notEqual(tick.layer, model.layerCount - 1, "the last Layer has no successor to drop it");
  }
  // Ticks are drawn, not omitted: every node is one mark.
  assert.equal(
    ofKind(model, "state").length + ticks.length,
    model.nodes.length,
    "every state is exactly one node or one tick",
  );
});

test("the last Layer is drawn as states with an end cap, because those are the boards worth clicking", () => {
  const model = tree();
  const last = model.nodes.filter((node) => node.layer === model.layerCount - 1);
  assert.ok(last.length > 0, "the last Layer is drawn");
  for (const node of last) {
    assert.equal(node.endCap, true, `${node.id} carries the end-of-trace cap`);
    assert.equal(node.deadEnd, false, "a state with no successor Layer is not a dead end");
    assert.equal(node.selectable, true, "and it can be clicked");
  }
  assert.equal(ofKind(model, "tick").filter((tick) => tick.layer === model.layerCount - 1).length, 0);
});

// ── the chevron: children the bound cut ───────────────────────────────────

test("a state whose children the bound cut is a chevron, not a tick", () => {
  const model = tree();
  const chevrons = ofKind(model, "chevron");
  assert.equal(chevrons.length, FIXTURE.expect.tree.chevrons, "the chevron count moved");
  for (const chevron of chevrons) {
    const node = model.nodes.find((candidate) => candidate.id === chevron.id);
    assert.ok(node.kidsInTrace > 0, `${chevron.id} has no child at all, so it is a tick, not a chevron`);
    assert.equal(node.kidsInWindow, 0, `${chevron.id} has a drawn child, so nothing was cut`);
    assert.equal(node.cut, true);
  }
  const tickIds = ids(ofKind(model, "tick"));
  const chevronIds = ids(chevrons);
  assert.deepEqual(
    tickIds.filter((id) => chevronIds.includes(id)),
    [],
    "the two marks are disjoint: one says the search dropped it, the other says the drawing did",
  );
  const noDrawnChild = model.nodes.filter((node) => node.kidsInWindow === 0 && node.layer < model.layerCount - 1);
  assert.equal(noDrawnChild.length, tickIds.length + chevronIds.length, "every state with no drawn child carries exactly one of the two marks");
  assert.equal(noDrawnChild.length, 189);
});

// ── the dropped edge: a stem, and the invariant that says how many ────────

test("an edge whose parent is not drawn is a stem, never a line to the column edge", () => {
  const model = tree();
  const stems = ofKind(model, "stem");
  assert.equal(model.edges.length, FIXTURE.expect.tree.edges, "the drawn-edge count moved");
  assert.equal(model.edges.length, 414);
  assert.equal(stems.length, FIXTURE.expect.tree.stems, "the stem count moved");
  assert.equal(stems.length, 12);
  assert.deepEqual(model.counts.perLayerStems, FIXTURE.expect.tree.perLayerStems);

  // The invariant: every non-root state is either reached by a drawn edge or
  // marked with a stem — exactly once. (The fixture's band is its beam, so no
  // parent is unrecorded; `-2` is asserted on a synthetic trace below.)
  const nonRoot = model.nodes.filter((node) => node.layer > 0).length;
  assert.equal(nonRoot, expectedDrawn(24).slice(1).reduce((sum, count) => sum + count, 0));
  assert.equal(model.edges.length + stems.length, nonRoot, "edges + stems === every non-root state");
  assert.equal(
    model.edges.length + stems.length,
    model.counts.perLayerDrawn.reduce((sum, count) => sum + count, 0) - model.counts.perLayerDrawn[0],
  );

  // Every edge joins two states that are in the drawing, and no edge enters layer 0.
  const drawn = new Set(model.nodes.map((node) => node.id));
  for (const edge of model.edges) {
    assert.ok(drawn.has(edge.from.id), `edge ${edge.id} starts at a drawn state`);
    assert.ok(drawn.has(edge.to.id), `edge ${edge.id} ends at a drawn state`);
    assert.ok(edge.to.layer > 0, "nothing enters layer 0");
    assert.equal(edge.from.layer, edge.to.layer - 1, "an edge joins consecutive Layers");
  }
  assert.equal(model.nodes.filter((node) => node.layer === 0).length, 1, "the root is one state");
  assert.equal(model.edges.filter((edge) => edge.to.layer === 0).length, 0);

  // A stem names the parent it had to drop, and that parent really is off-view.
  for (const stem of stems) {
    const node = model.nodes.find((candidate) => candidate.id === stem.id);
    assert.equal(node.parent, null, `${stem.id} has a drawn parent, so it needs no stem`);
    assert.equal(node.orphan.reason, "outside-bound");
    assert.ok(
      node.orphan.hiddenParentRank >= model.counts.perLayerDrawn[node.layer - 1],
      `${stem.id} names parent ${node.orphan.hiddenParentRank}, but the previous column draws ${model.counts.perLayerDrawn[node.layer - 1]}`,
    );
    assert.equal(node.orphan.hiddenParentRank, layerOf(node.layer)[node.rank].parentRank, "the stem prints the record's own parent_rank");
  }

  // And the complement: with the whole beam drawn, nothing is dropped.
  const whole = tree({ bound: FIXTURE.band });
  assert.equal(whole.counts.perLayerStems.reduce((sum, count) => sum + count, 0), 0, "band === beam drops no edge");
  assert.equal(whole.edges.length, TRACE.records.length - 1, "every non-root record is reached");
});

test("the -2 sentinel: the edge is dropped and the stem says the trace did not record the parent", () => {
  const clipped = synthetic([
    [{ rank: 0, score: -100, lines: 0, parentRank: -1 }],
    [
      { rank: 0, score: -101, lines: 0, parentRank: 0 },
      { rank: 1, score: -102, lines: 0, parentRank: 0 },
    ],
    [
      { rank: 0, score: -103, lines: 0, parentRank: 0 },
      { rank: 1, score: -104, lines: 0, parentRank: -2 },
    ],
  ]);
  const model = buildTree(clipped, { ...PLOT, bound: 24 });
  const unrecorded = model.nodes.find((node) => node.layer === 2 && node.rank === 1);
  assert.equal(unrecorded.parent, null, "a -2 parent is never resolved to a state");
  assert.equal(unrecorded.orphan.reason, "unrecorded", "and the stem says which kind of hole it is");
  assert.equal(unrecorded.orphan.hiddenParentRank, -2, "the sentinel is carried, not laundered into a rank");
  assert.equal(
    model.edges.filter((edge) => edge.to.layer === 2 && edge.to.rank === 1).length,
    0,
    "no edge is drawn for it — not to rank 0, not to the column edge",
  );
  const stems = ofKind(model, "stem");
  assert.equal(stems.length, 1, "the dropped edge is drawn as one stem");
  assert.equal(stems[0].id, "2:1");
  // The two kinds of stem are told apart in words, so the drawing never implies
  // a parent that exists outside the cut when the trace simply did not keep it.
  assert.match(TREE_COPY.stemOutsideBound, /outside the top/);
  assert.match(TREE_COPY.stemUnrecorded, /not recorded/);
  assert.notEqual(TREE_COPY.stemOutsideBound, TREE_COPY.stemUnrecorded);

  // The invariant holds in the clipped regime too: dropped edges are stems, and
  // stems are the only states without a drawn parent.
  const nonRoot = model.nodes.filter((node) => node.layer > 0).length;
  assert.equal(model.edges.length + stems.length, nonRoot);

  // A parent beyond the cut is the other reason, and it is not the same words.
  const beyond = buildTree(clipped, { ...PLOT, bound: 1 });
  const cutAway = beyond.nodes.find((node) => node.layer === 1 && node.rank === 0);
  assert.equal(cutAway.orphan, null, "rank 0's parent is rank 0, which bound 1 still draws");
  const wide = synthetic([
    [{ rank: 0, score: -100, lines: 0, parentRank: -1 }],
    [
      { rank: 0, score: -101, lines: 0, parentRank: 0 },
      { rank: 1, score: -102, lines: 0, parentRank: 0 },
    ],
    [{ rank: 0, score: -103, lines: 0, parentRank: 1 }],
  ]);
  const narrow = buildTree(wide, { ...PLOT, bound: 1 });
  const offCut = narrow.nodes.find((node) => node.layer === 2 && node.rank === 0);
  assert.equal(offCut.orphan.reason, "outside-bound");
  assert.equal(offCut.orphan.hiddenParentRank, 1);
  assert.equal(
    narrow.edges.filter((edge) => edge.to.layer === 2).length,
    0,
    "bound 1 on a 2-state Layer draws no edge into layer 2",
  );
});

// ── selection: the dim, and the path ──────────────────────────────────────

test("selecting a state dims everything off its path", () => {
  const selected = { layer: TRACE.layerCount - 1, rank: 0 };
  const model = tree({ selection: selected });
  const ancestry = ancestryOf(TRACE, selected);
  const descent = descentOf(TRACE, selected);
  const path = new Set([...ancestry, ...descent].map((ref) => `${ref.layer}:${ref.rank}`));
  // Both relations include the selection, so they meet exactly there.
  assert.equal(path.size, ancestry.length + descent.length - 1, "the two relations meet at the selection and nowhere else");
  assert.equal(ancestry.length, TRACE.layerCount, "the ancestry of the best final state is one state per Layer");
  assert.equal(descent.length, 1, "the best final state has no children: it is the last Layer");
  assert.deepEqual(descent, [selected], "the descent is the selection itself when it has no children");
  assert.equal(ancestry[0].layer, 0, "the ancestry runs back to the root");

  for (const node of model.nodes) {
    const onPath = path.has(node.id);
    assert.equal(node.onPath, onPath, `${node.id} on the path`);
    assert.equal(node.dimmed, !onPath, `${node.id} is dimmed exactly when it is off the path`);
    assert.equal(node.alpha, onPath ? TREE_ALPHA.onPath : TREE_ALPHA.offPathState, `${node.id} alpha`);
  }
  assert.equal(model.nodes.filter((node) => !node.dimmed).length, path.size, "the full-weight set is the path");

  // The bug not to copy from the prototype: its dim left the edges at full
  // strength, so a 200-state web survived the dim.
  for (const edge of model.edges) {
    const onPath = path.has(edge.from.id) && path.has(edge.to.id);
    assert.equal(edge.onPath, onPath, `edge ${edge.id}`);
    assert.equal(edge.dimmed, !onPath, `edge ${edge.id} must dim with the states it joins`);
    assert.equal(edge.alpha, onPath ? TREE_ALPHA.onPath : TREE_ALPHA.offPathEdge, `edge ${edge.id} alpha`);
  }
  assert.ok(model.edges.filter((edge) => !edge.dimmed).length >= 1, "the path has edges");

  // Off-path marks dim too, and the lineage keeps a floor of its own so the
  // funnel is still readable around a selection.
  for (const mark of ofKind(model, "tick").concat(ofKind(model, "stem"), ofKind(model, "chevron"))) {
    const onPath = path.has(mark.id);
    assert.equal(mark.alpha, onPath ? TREE_ALPHA.onPath : TREE_ALPHA.offPathMark, `${mark.id} mark alpha`);
  }
  for (const node of model.nodes) {
    if (!node.onLineage) continue;
    assert.equal(node.lineageAlpha, node.onPath ? TREE_ALPHA.onPath : TREE_ALPHA.offPathLineage);
  }

  // No selection, no dim: the picture is the whole funnel.
  const unselected = tree();
  assert.equal(unselected.selection, null);
  assert.ok(unselected.nodes.every((node) => node.dimmed === false));
  assert.ok(unselected.edges.every((edge) => edge.dimmed === false));
});

test("a click picks the nearest mark, and a tick is clickable", () => {
  const model = tree();
  const tick = ofKind(model, "tick")[0];
  const picked = pickState(model, tick.x, tick.y);
  assert.deepEqual(picked, { layer: tick.layer, rank: tick.rank }, "a click on a tick selects that state");
  const node = ofKind(model, "state")[3];
  assert.deepEqual(pickState(model, node.x + 1, node.y - 1), { layer: node.layer, rank: node.rank });
  assert.equal(pickState(model, -100, -100), null, "a click on nothing selects nothing");
  assert.ok(model.layout.hitRadius >= 6, "a 4-px dot still has a hit target");
});

// ── the two numbers: the tree's bound and the beam width ──────────────────

test("the tree's bound is a separate control from the beam width", () => {
  // Two integers, two setters: moving the bound rebuilds the picture from the
  // trace already in memory and touches nothing else.
  const at24 = tree();
  const at40 = tree({ bound: 40 });
  const at60 = tree({ bound: 60 });
  assert.equal(at24.nodes.length, 427);
  assert.equal(at40.nodes.length, FIXTURE.expect.tree.nodesAtBound["40"]);
  assert.equal(at60.nodes.length, FIXTURE.expect.tree.nodesAtBound["60"]);
  assert.equal(at40.bound, 40);
  assert.equal(at24.bound, 24);
  // The band — the beam's own record cap — is the same number in all three: the
  // tree's bound never rewrites it, in either direction.
  for (const model of [at24, at40, at60]) {
    assert.equal(model.band, FIXTURE.band, "the band is the trace's, whatever the tree's bound is");
    assert.equal(model.records, TRACE.records.length, "and the trace is not re-read");
  }
  // The label prints the measurement, not a bare number (§2.1 item 2).
  assert.match(TREE_COPY.boundLabel, /tree/i);
  assert.match(TREE_COPY.boundTooltip, /does not change the beam/);
  assert.match(TREE_COPY.boundTooltip, /does not change the search/);
});

test("the tree is a pure function of the trace: no solver, no worker, no mutation", () => {
  const frozen = deepFreeze(JSON.parse(JSON.stringify(TRACE)));
  const before = JSON.stringify(frozen);
  const first = buildTree(frozen, { ...PLOT, bound: 24, selection: { layer: 3, rank: 1 } });
  const second = buildTree(frozen, { ...PLOT, bound: 24, selection: { layer: 3, rank: 1 } });
  assert.deepEqual(first, second, "same trace, same picture");
  assert.equal(JSON.stringify(frozen), before, "the model never writes to the trace it was handed");

  // The seam, asserted the cheap way the notes prescribe: the model's whole
  // dependency list is the decoder, so there is no path from here to wasm, to
  // the worker, or to a re-solve.
  for (const file of ["tree-model.ts", "tree-canvas.ts"]) {
    const source = readFileSync(fileURLToPath(new URL(`../src/ui/${file}`, import.meta.url)), "utf8");
    const specifiers = [...source.matchAll(/from\s+"([^"]+)"/g)].map((match) => match[1]);
    for (const specifier of specifiers) {
      assert.ok(
        !/core\/|workers\/|\.wasm|solve_traced/.test(specifier),
        `${file} imports ${specifier}; the tree must reach nothing but the trace`,
      );
      assert.ok(specifier.startsWith("./"), `${file} imports ${specifier} from outside web/src/ui`);
    }
  }
});

// ── the selected state's Board ────────────────────────────────────────────

test("the state card shows that state's Board when the trace can supply it, and says so when it cannot", () => {
  const rootBoard = FIXTURE.game.board;
  const boardAt = (layer, rank) => (layer === 0 && rank === 0 ? rootBoard : null);
  const model = tree({ selection: { layer: 0, rank: 0 } });

  const root = stateCard(model, { layer: 0, rank: 0 }, { boardAt });
  assert.deepEqual(root.board, rootBoard, "layer 0's state is the game's own starting Board");
  assert.equal(root.boardNote, null, "a Board that is shown needs no apology");
  assert.equal(root.layer, 0);
  assert.equal(root.rank, 0);
  assert.equal(root.score, FIXTURE.expect.rootScore);
  assert.equal(root.piece, "root");
  assert.match(root.facts, /layer 0/);
  assert.match(root.facts, /rank 0 of 1/);
  assert.match(root.facts, /score -111/);

  // §4.4's five words cannot yield an arbitrary state's Board, so the card says
  // so in words rather than drawing a plausible-looking wrong board (§2.1 item 1).
  const elsewhere = stateCard(model, { layer: 7, rank: 3 }, { boardAt });
  assert.equal(elsewhere.board, null);
  assert.match(elsewhere.boardNote, /not recorded in the trace/);
  assert.equal(elsewhere.score, layerOf(7)[3].score, "the facts are still the record's own");
  assert.equal(elsewhere.lines, layerOf(7)[3].lines);
  assert.match(elsewhere.facts, /rank 3 of 25/);
});

test("the card's Board is drawn at a size where a one-cell height difference is visible", () => {
  const wide = stateCardLayout(614);
  assert.equal(wide.cell, 14, "at 2560×1440 the cell is 14 px");
  assert.equal(wide.width, 140);
  assert.equal(wide.height, 280);
  assert.ok(wide.cell >= 8, "below an 8-px cell a one-cell step stops reading as a step");
  const narrow = stateCardLayout(420);
  assert.ok(narrow.cell <= wide.cell && narrow.cell >= 8, "a narrower inspector shrinks the card, never the tree");
  assert.equal(narrow.cell * 10, narrow.width);
  assert.equal(narrow.cell * 20, narrow.height);
});

// ── the words on screen ───────────────────────────────────────────────────

test("the words on screen call the band the Beam and the tree the Tree", () => {
  assert.deepEqual(TREE_COPY.views, ["Beam", "Tree"], "the two views, in the ticket's vocabulary");
  assert.match(TREE_COPY.title, /^the tree/);
  assert.match(TREE_COPY.title, /top 24/);
  for (const word of TREE_COPY.views) assert.ok(!/band/i.test(word), "the band is never a view's name");
  // GLOSSARY's *Avoid* list for Evaluation bans "node", so a mark is a state.
  assert.ok(!/node/i.test(TREE_COPY.legend.join(" ")), "the legend calls a mark a state, not a node");
  assert.match(TREE_COPY.legend.join(" "), /state/);
  assert.match(TREE_COPY.legend.join(" "), /tick/);
  assert.match(TREE_COPY.legend.join(" "), /chevron/);
  assert.match(TREE_COPY.legend.join(" "), /stem/);
  assert.match(TREE_COPY.emptyTrace, /no trace/);
});

test("the geometry keeps 24 rows inside the inspector at the display the ticket names", () => {
  const model = tree();
  assert.equal(model.layout.rowPitch, 31, "24 rows at a 31-px pitch inside a 786-px plot");
  assert.equal(model.layout.nodeDiameter, 8);
  assert.ok(model.layout.rowPitch * 24 <= model.layout.plotHeight, "24 rows fit");
  assert.equal(model.window.count, model.layerCount, "the default game's 20 Layers fit one window");
  assert.equal(model.window.total, model.layerCount);
  const dense = tree({ width: 420, height: 306 });
  assert.ok(dense.layout.rowPitch < model.layout.rowPitch, "a tighter inspector tightens the pitch");
  assert.ok(dense.layout.rowPitch * 24 <= dense.layout.plotHeight);
  // The Layer window: a 101-Layer game cannot be a grate.
  const long = synthetic(Array.from({ length: 101 }, (_, layer) => [{ rank: 0, score: -100 - layer, lines: 0, parentRank: layer === 0 ? -1 : 0 }]));
  const windowed = buildTree(long, { width: 420, height: 306, bound: 24 });
  assert.ok(windowed.window.count < windowed.window.total, "a 101-Layer trace is windowed");
  assert.ok(windowed.layout.colPitch >= 14, "columns never fall below the 14-px floor");
  const centred = buildTree(long, { width: 420, height: 306, selection: { layer: 90, rank: 0 } });
  assert.ok(
    centred.window.from <= 90 && 90 < centred.window.from + centred.window.count,
    "the window follows the selection",
  );
});

// ── helpers ───────────────────────────────────────────────────────────────

function deepFreeze(value) {
  if (value !== null && typeof value === "object") {
    for (const key of Object.keys(value)) deepFreeze(value[key]);
    Object.freeze(value);
  }
  return value;
}
