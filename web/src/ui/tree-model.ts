// The bounded tree as a pure model: states, edges, marks and the dim — no DOM, no
// canvas, no wasm, no worker.
//
// §2.3's drill-down, as a data structure: "**on demand: a bounded tree**, top 24
// states per layer (≈360 nodes for a 40-piece game), childless states as ticks,
// dim-off-selection, board preview on click." This module is that sentence, and
// `web/src/ui/tree-canvas.ts` draws nothing it did not compute, so
// `web/test/tree.test.mjs` can assert the whole picture with no browser at all.
//
// Its one input is ticket 11's decoded trace (`ui/trace.ts`). That is the whole
// point: the tree is a *projection* of a recording the engine already made, not a
// second search. There is no scoring here, no expansion, no Board and — the
// assertion at the bottom of the test file — no sort. `rank` is 0-based within its
// Layer with 0 the best score (§4.4), the records arrive in layer-then-rank order,
// and so the top `bound` states of a Layer are its first `bound` records. Sorting
// them in JavaScript would be a second implementation of the engine's ordering that
// silently disagrees on ties.
//
// Five rules, each a claim the ticket makes:
//
// 1. **At most `bound` states per Layer** (24 by default, the measured legibility
//    bound), top-anchored at rank 0 and *ragged*: a Layer with fewer records is
//    drawn as it is, never padded and never stretched.
// 2. **A state with no child in the trace is a tick**, not an empty node. At a
//    band-25 trace 44 % of the drawn states are dead ends — that is the mark that
//    keeps the picture from being a field of dots.
// 3. **A state whose children exist but were cut by the bound is a chevron**, and
//    one whose *parent* was cut is a stem. The two say different things — "the
//    search dropped it" vs "the drawing did" — and both are needed: the bound, not
//    the search, is what hides most of the funnel's children.
// 4. **An edge is drawn only when both ends are drawn.** The dropped ones are
//    stems that carry the hidden parent's rank, including §4.4's `-2` sentinel,
//    where the honest words are "the trace did not record the parent" — never a
//    line to the column edge as though the parent were rank 0.
// 5. **The bound is the tree's own number.** It is not the beam width, it is not in
//    the URL, and changing it re-renders from the trace already in memory: zero
//    solves, zero worker messages, zero wasm.
//
// The invariant, which the test asserts in the arithmetic form:
//
//   *every edge drawn joins two states that are in the node list; every non-root
//   state without a drawn edge is marked exactly once as a stem (its parent existed
//   and is off-view, or the trace did not record it); and nothing enters layer 0.*
//
//   edges.length + stems.length === Σ_{layer > 0} drawn(layer)
//
// At `bound === band` the right-hand side is the trace's own record count minus the
// root, and `stems.length === 0`: the same identities collapse, which is what makes
// the two regimes one implementation.

import {
  PARENT_NONE,
  PARENT_UNRECORDED,
  layerSlice,
  recordAt,
  type Trace,
  type TraceRecord,
} from "./trace.ts";
import { winningLineage } from "./band-model.ts";

// ── the bound, and the words ────────────────────────────────────────────────

/** The measured legibility bound: top 24 states per Layer (§2.3). */
export const TREE_BOUND_DEFAULT = 24;
/** Below this a Layer is a dot: 4 rows is the smallest column that reads as a column. */
export const TREE_BOUND_MIN = 4;
/** The haze: `FINDINGS.md` measured top 60 as "busy; the crossing haze competes". */
export const TREE_BOUND_MAX = 60;
/** Two rows per notch, so 24 is reachable from either side. */
export const TREE_BOUND_STEP = 2;

/**
 * The dim, as opacities.
 *
 * The prototype's dim ink was `rgba(70,80,88,0.30)` for states, measured at 200 of
 * them. At 24 states with an 8-px dot 0.30 still reads as a field of dots, so the
 * off-path fill drops to 0.14. The prototype's real bug is not copied: its edge
 * loop emitted full-strength ink regardless of the dim, so "dim everything off the
 * selection" left a web of edges behind. Every off-path edge goes to 0.05.
 */
export const TREE_ALPHA = {
  onPath: 1,
  /** A state off the selected path. */
  offPathState: 0.14,
  /** An edge off it — the value the prototype got wrong. */
  offPathEdge: 0.05,
  /** A tick, chevron or stem off it. */
  offPathMark: 0.1,
  /** The winning lineage, when the selection is somewhere else: dimmed, not gone. */
  offPathLineage: 0.45,
} as const;

/**
 * Every word the tree puts on screen, in one place.
 *
 * Two vocabulary rules from §2.3 are load-bearing here: the band **is** the beam,
 * so the default view's name is "Beam" and "band" is never a view's name; and
 * `GLOSSARY.md` bans "node" (under *Evaluation*: "_Avoid_: node, sample,
 * iteration"), so a mark is a **state**.
 */
export const TREE_COPY = {
  /** The view toggle, in the ticket's vocabulary. */
  views: ["Beam", "Tree"] as const,
  /** The pane header. The live line under it carries the numbers. */
  title: "the tree · top 24 per Layer",
  boundLabel: "the tree's bound — how many of each Layer's best states are drawn",
  boundTooltip:
    "the tree's bound — how many of each Layer's best states are drawn. It does not change the beam, and it does not change the search.",
  /** Four marks are four figures, so each is named (§2.1 item 2). */
  legend: [
    "a state is a surviving board",
    "a tick = no child, the search dropped it",
    "a chevron = its children are below the bound",
    "a stem = its parent is",
  ] as string[],
  /** The stem's tooltip when the parent exists but the bound cut it. */
  stemOutsideBound: "parent rank {rank} — outside the top {bound}",
  /** The stem's tooltip when the trace never recorded the parent (§4.4's `-2`). */
  stemUnrecorded: "its parent was not recorded in the trace — the band cut it before it reached the beam",
  /** The stem's tooltip when the parent's Layer is left of the Layer window. */
  stemOutsideWindow: "parent rank {rank} — its Layer is left of the window",
  emptyTrace: "no trace yet — solve a game to draw the tree",
  axis: "Layer / piece — the beam's stages, left to right",
} as const;

/** The tree bound's own range: `4 … min(60, band)`, step 2. */
export function treeBoundLimit(band: number): number {
  return Math.max(1, Math.min(TREE_BOUND_MAX, Math.floor(band)));
}

/** Clamp a bound into `[4, min(60, band)]`; the band is the hard ceiling. */
export function clampTreeBound(bound: number, band: number): number {
  const limit = treeBoundLimit(band);
  const floor = Math.min(TREE_BOUND_MIN, limit);
  const value = Number.isFinite(bound) ? Math.round(bound) : TREE_BOUND_DEFAULT;
  return Math.max(floor, Math.min(limit, value));
}

// ── the geometry ───────────────────────────────────────────────────────────

/** Pixels of left gutter: the rank labels and the orphan stems enter here. */
const GUTTER_LEFT = 46;
/** Pixels of right gutter: the chevrons sit at the column's right edge. */
const GUTTER_RIGHT = 12;
/** The ruler band above the columns (`L4 T`). */
const RULER = 22;
/** The axis band under them. */
const AXIS = 18;
/**
 * The narrowest column whose edges still separate: a 4-px node plus a 10-px gap.
 * A 101-Layer game at a 420-px inspector would be 5.3 px of pitch without it, so
 * this floor is what turns the drawing into a **window** instead of a grate.
 */
const COLUMN_FLOOR = 14;

export interface TreeLayout {
  /** CSS pixels of the canvas the model was laid out for. */
  width: number;
  height: number;
  plotWidth: number;
  plotHeight: number;
  originX: number;
  originY: number;
  /** Horizontal pitch between Layer columns. */
  colPitch: number;
  /** Vertical pitch between rank rows — the number the legibility bound hangs on. */
  rowPitch: number;
  /** Node diameter: `clamp(4, round(0.35 × rowPitch), 8)`. */
  nodeDiameter: number;
  /** The radius a click resolves within: `max(6, rowPitch / 2)`. */
  hitRadius: number;
  /** Columns that fit at the 14-px floor. */
  maxColumns: number;
  /** Rows in the tallest drawn column. */
  rows: number;
  /** The smallest cell at which a one-cell column-height step still reads. */
  minCell: number;
}

export interface TreeColumn {
  layer: number;
  /** The piece this Layer placed (`sequence[layer - 1]`), or `root` at layer 0. */
  piece: string;
  /** The column's centre, in CSS pixels. */
  x: number;
  /** Records the Layer kept — the engine's count, never a recount. */
  count: number;
  /** States drawn in this column: `min(bound, count)`. */
  drawn: number;
  /** The ruler label, e.g. `L4 T`. */
  label: string;
}

export interface TreeRef {
  layer: number;
  rank: number;
}

export type TreeOrphanReason = "outside-bound" | "unrecorded" | "outside-window";

export interface TreeOrphan {
  /** The record's own `parent_rank`: a rank off-view, or `-2` when unrecorded. */
  hiddenParentRank: number;
  reason: TreeOrphanReason;
  /** The words for the tooltip. */
  words: string;
}

export interface TreeNode {
  /** `${layer}:${rank}` — a reference into the trace, not a copy of it. */
  id: string;
  layer: number;
  rank: number;
  score: number;
  lines: number;
  piece: string;
  /** Which primary mark draws it. */
  mark: "state" | "tick";
  /** No child anywhere in the trace, and a next Layer exists to drop it. */
  deadEnd: boolean;
  /** Has children in the trace, none inside the bound: a chevron, not a tick. */
  cut: boolean;
  /** The last recorded Layer: no successor to drop it, so a state with an end cap. */
  endCap: boolean;
  kidsInTrace: number;
  kidsInWindow: number;
  /** The parent, when it is drawn. */
  parent: TreeRef | null;
  /** The parent the drawing had to drop, and why. */
  orphan: TreeOrphan | null;
  onLineage: boolean;
  /** `true` when the state is on the selection's path, or when nothing is selected. */
  onPath: boolean;
  dimmed: boolean;
  /** The state mark's own opacity (a tick's is `TREE_ALPHA.offPathMark`). */
  alpha: number;
  /** The lineage's opacity, when this state is on it. */
  lineageAlpha: number;
  selected: boolean;
  selectable: boolean;
  /** Column centre, row centre, in CSS pixels. */
  x: number;
  y: number;
  /** One line for a tooltip: the record's own numbers, in words. */
  tooltip: string;
}

export interface TreeEdge {
  id: string;
  from: TreeRef;
  to: TreeRef;
  onPath: boolean;
  dimmed: boolean;
  alpha: number;
  /** Both ends are on the winning lineage, so the edge is drawn amber. */
  lineage: boolean;
  x1: number;
  y1: number;
  x2: number;
  y2: number;
}

export interface TreeCounts {
  /** States drawn per Layer: `min(bound, perLayerCounts[layer])`. */
  perLayerDrawn: number[];
  /** The Layer's records, the engine's count. */
  perLayerRecords: number[];
  perLayerTicks: number[];
  perLayerChevrons: number[];
  perLayerStems: number[];
  /** The states the engine kept, at most `band` per Layer. */
  records: number;
}

export interface TreeModel {
  bound: number;
  band: number;
  layerCount: number;
  records: number;
  nodes: TreeNode[];
  edges: TreeEdge[];
  columns: TreeColumn[];
  /** The Layer window: a 101-Layer game is not a grate. */
  window: { from: number; count: number; total: number };
  selection: TreeRef | null;
  /** The winning lineage's rank per Layer, for the amber path. */
  lineage: TreeRef[];
  counts: TreeCounts;
  /** Layers whose records were not score-descending. Reported, never repaired. */
  orderViolation: number[];
  layout: TreeLayout;
  /** One line for the inspector, with the numbers in it. */
  label: string;
  caption: string;
}

export interface TreeOptions {
  /** States per Layer. Defaults to `TREE_BOUND_DEFAULT`; clamped to the band. */
  bound?: number;
  /** The selected state, whose path is drawn at full weight. */
  selection?: TreeRef | null;
  /** The canvas the model is laid out for, in CSS pixels. */
  width?: number;
  height?: number;
  /** The Sequence, so a column can be labelled with the piece it placed. */
  sequence?: string;
  /** Force the Layer window instead of centring it on the selection. */
  window?: { from: number; count: number };
}

// ── the model ──────────────────────────────────────────────────────────────

/** For each Layer, `rank -> the ranks of its children in the next Layer`. */
function childrenIndex(trace: Trace): Array<Map<number, number[]>> {
  const index: Array<Map<number, number[]>> = [];
  for (let layer = 0; layer + 1 < trace.layerCount; layer += 1) {
    const children = new Map<number, number[]>();
    for (const record of layerSlice(trace, layer + 1)) {
      const parent = record.parentRank;
      if (parent < 0) continue;
      const bucket = children.get(parent);
      if (bucket === undefined) children.set(parent, [record.rank]);
      else bucket.push(record.rank);
    }
    index[layer] = children;
  }
  return index;
}

/** The state's ancestors, root first, ending at the state itself. */
export function ancestryOf(trace: Trace, ref: TreeRef): TreeRef[] {
  const path: TreeRef[] = [];
  let layer = ref.layer;
  let rank = ref.rank;
  for (;;) {
    if (layer < 0 || recordAt(trace, layer, rank) === undefined) break;
    path.push({ layer, rank });
    if (layer === 0) break;
    const parent = recordAt(trace, layer, rank)!.parentRank;
    if (parent < 0) break;
    layer -= 1;
    rank = parent;
  }
  return path.reverse();
}

/** The state and everything under it that the trace recorded, depth-first by rank. */
export function descentOf(trace: Trace, ref: TreeRef): TreeRef[] {
  if (recordAt(trace, ref.layer, ref.rank) === undefined) return [];
  const index = childrenIndex(trace);
  const out: TreeRef[] = [];
  const stack: TreeRef[] = [ref];
  while (stack.length > 0) {
    const at = stack.pop()!;
    out.push(at);
    const children = index[at.layer]?.get(at.rank);
    if (children === undefined) continue;
    for (let i = children.length - 1; i >= 0; i -= 1) stack.push({ layer: at.layer + 1, rank: children[i] });
  }
  return out;
}

function pieceAt(sequence: string | undefined, layer: number): string {
  if (layer === 0) return "root";
  if (sequence === undefined || layer - 1 >= sequence.length) return "?";
  return sequence[layer - 1];
}

function plural(count: number, noun: string): string {
  return `${count} ${noun}${count === 1 ? "" : "s"}`;
}

/**
 * Build the whole picture from a decoded trace. Pure: the trace is not mutated,
 * nothing is solved, and the same inputs always give the same model.
 */
export function buildTree(trace: Trace, options: TreeOptions = {}): TreeModel {
  const band = trace.band;
  const bound = clampTreeBound(options.bound ?? TREE_BOUND_DEFAULT, band);
  const width = Math.max(1, Math.floor(options.width ?? 590));
  const height = Math.max(1, Math.floor(options.height ?? 786));

  const perLayerRecords = trace.layerCount === 0 ? [] : [...trace.perLayerCounts];
  const perLayerDrawn = perLayerRecords.map((count) => Math.min(bound, count));
  const rows = Math.max(1, ...perLayerDrawn);

  // The Layer window: every Layer when they fit, else a run centred on the
  // selection. 14 px is the floor below which a column stops being a column.
  const plotWidth = Math.max(1, width - GUTTER_LEFT - GUTTER_RIGHT);
  const plotHeight = Math.max(1, height - RULER - AXIS);
  const maxColumns = Math.max(1, Math.floor(plotWidth / COLUMN_FLOOR));
  const total = trace.layerCount;
  const count = Math.min(total, Math.max(1, options.window?.count ?? maxColumns));
  const centre = options.selection?.layer ?? 0;
  const wanted = options.window?.from ?? Math.max(0, centre - Math.floor(count / 2));
  const from = Math.max(0, Math.min(Math.max(0, total - count), Math.floor(wanted)));
  const drawnColumns = Math.max(0, Math.min(count, total - from));
  const colPitch = drawnColumns > 1 ? plotWidth / (drawnColumns - 1) : 0;
  const rowPitch = Math.max(1, Math.floor(plotHeight / rows));
  const layout: TreeLayout = {
    width,
    height,
    plotWidth,
    plotHeight,
    originX: GUTTER_LEFT,
    originY: RULER,
    colPitch,
    rowPitch,
    nodeDiameter: Math.max(4, Math.min(8, Math.round(0.35 * rowPitch))),
    hitRadius: Math.max(6, rowPitch / 2),
    maxColumns,
    rows,
    minCell: 8,
  };

  const xOf = (layer: number): number => {
    const index = layer - from;
    if (drawnColumns <= 1) return GUTTER_LEFT + plotWidth / 2;
    return GUTTER_LEFT + Math.min(drawnColumns - 1, Math.max(0, index)) * colPitch;
  };
  const yOf = (rank: number): number => RULER + rank * rowPitch + rowPitch / 2;

  // ── child counts: one linear pass, never a search ────────────────────────
  const kidsInTrace: number[][] = [];
  const kidsInWindow: number[][] = [];
  for (let layer = 0; layer + 1 < trace.layerCount; layer += 1) {
    kidsInTrace[layer] = new Array(perLayerRecords[layer]).fill(0);
    kidsInWindow[layer] = new Array(perLayerDrawn[layer]).fill(0);
  }
  for (let layer = 1; layer < trace.layerCount; layer += 1) {
    for (const record of layerSlice(trace, layer)) {
      const parent = record.parentRank;
      if (parent < 0 || parent >= perLayerRecords[layer - 1]) continue;
      kidsInTrace[layer - 1][parent] += 1;
      // A child counts towards its parent's *drawn* children only when the child
      // itself is drawn: a rank past the bound is a child the chevron speaks for.
      if (record.rank < perLayerDrawn[layer] && parent < perLayerDrawn[layer - 1]) {
        kidsInWindow[layer - 1][parent] += 1;
      }
    }
  }

  // ── the order guard: report a Layer that is not score-descending ─────────
  const orderViolation: number[] = [];
  for (let layer = 1; layer < trace.layerCount; layer += 1) {
    const records = layerSlice(trace, layer);
    for (let i = 1; i < records.length; i += 1) {
      if (records[i].score > records[i - 1].score) {
        orderViolation.push(layer);
        break;
      }
    }
  }

  // ── the lineage, and the selection's path ────────────────────────────────
  const lineage: TreeRef[] = winningLineage(trace).map((point) => ({ layer: point.layer, rank: point.rank }));
  const lineageSet = new Set(lineage.map((ref) => `${ref.layer}:${ref.rank}`));
  const selection = options.selection ?? null;
  let pathSet: Set<string> | null = null;
  if (selection !== null) {
    pathSet = new Set<string>();
    for (const ref of ancestryOf(trace, selection)) pathSet.add(`${ref.layer}:${ref.rank}`);
    for (const ref of descentOf(trace, selection)) pathSet.add(`${ref.layer}:${ref.rank}`);
  }
  const onPathOf = (id: string): boolean => (pathSet === null ? true : pathSet.has(id));

  // ── the nodes ───────────────────────────────────────────────────────────
  const nodes: TreeNode[] = [];
  const byId = new Map<string, TreeNode>();
  const perLayerTicks = new Array(Math.max(0, trace.layerCount)).fill(0);
  const perLayerChevrons = new Array(Math.max(0, trace.layerCount)).fill(0);
  const perLayerStems = new Array(Math.max(0, trace.layerCount)).fill(0);
  const lastLayer = trace.layerCount - 1;

  for (let layer = from; layer < from + drawnColumns; layer += 1) {
    for (let rank = 0; rank < perLayerDrawn[layer]; rank += 1) {
      const record = recordAt(trace, layer, rank) as TraceRecord;
      const id = `${layer}:${rank}`;
      const kidsTrace = layer < lastLayer ? kidsInTrace[layer][rank] : 0;
      const kidsWindow = layer < lastLayer ? kidsInWindow[layer][rank] : 0;
      const endCap = layer === lastLayer;
      const deadEnd = !endCap && kidsTrace === 0;
      const cut = !endCap && kidsTrace > 0 && kidsWindow === 0;

      let parent: TreeRef | null = null;
      let orphan: TreeOrphan | null = null;
      if (layer > 0) {
        const parentRank = record.parentRank;
        const previousDrawn = layer - 1 >= from;
        if (previousDrawn && parentRank >= 0 && parentRank < perLayerDrawn[layer - 1]) {
          parent = { layer: layer - 1, rank: parentRank };
        } else {
          // Three ways for a parent to be off-view, and the stem says which: the
          // trace never recorded it (`-2`), the bound cut its record, or its Layer
          // is outside the window. Never silently, and never a line to rank 0.
          const reason: TreeOrphanReason =
            parentRank < 0 ? "unrecorded" : !previousDrawn ? "outside-window" : "outside-bound";
          const words =
            reason === "unrecorded"
              ? TREE_COPY.stemUnrecorded
              : (reason === "outside-window" ? TREE_COPY.stemOutsideWindow : TREE_COPY.stemOutsideBound)
                  .replace("{rank}", String(parentRank))
                  .replace("{bound}", String(bound));
          orphan = { hiddenParentRank: parentRank, reason, words };
        }
      }

      const onPath = onPathOf(id);
      const onLineage = lineageSet.has(id);
      const selected = selection !== null && selection.layer === layer && selection.rank === rank;
      const piece = pieceAt(options.sequence, layer);
      const tooltipParts = [
        `layer ${layer} (${piece})`,
        `rank ${rank} of ${perLayerRecords[layer]}`,
        `score ${record.score}`,
        `${record.lines} lines`,
      ];
      if (endCap) tooltipParts.push("the last Layer — no successor to drop it");
      else if (deadEnd) tooltipParts.push("no child — the search dropped it");
      else if (cut) tooltipParts.push(`${plural(kidsTrace, "child")}, all below the top ${bound}`);
      else tooltipParts.push(`${plural(kidsTrace, "child")} (${kidsWindow} inside the top ${bound})`);
      if (orphan !== null) tooltipParts.push(orphan.words);

      const node: TreeNode = {
        id,
        layer,
        rank,
        score: record.score,
        lines: record.lines,
        piece,
        mark: deadEnd ? "tick" : "state",
        deadEnd,
        cut,
        endCap,
        kidsInTrace: kidsTrace,
        kidsInWindow: kidsWindow,
        parent,
        orphan,
        onLineage,
        onPath,
        dimmed: !onPath,
        alpha: onPath ? TREE_ALPHA.onPath : TREE_ALPHA.offPathState,
        lineageAlpha: onPath ? TREE_ALPHA.onPath : TREE_ALPHA.offPathLineage,
        selected,
        selectable: true,
        x: xOf(layer),
        y: yOf(rank),
        tooltip: tooltipParts.join(" · "),
      };
      nodes.push(node);
      byId.set(id, node);
      if (deadEnd) perLayerTicks[layer] += 1;
      if (cut) perLayerChevrons[layer] += 1;
      if (orphan !== null) perLayerStems[layer] += 1;
    }
  }

  // ── the edges: both ends drawn, or a stem instead ────────────────────────
  const edges: TreeEdge[] = [];
  for (const node of nodes) {
    if (node.parent === null) continue;
    const fromNode = byId.get(`${node.parent.layer}:${node.parent.rank}`);
    // `parent` is only set when the parent's column is drawn, so this cannot be
    // missing; skipping rather than drawing a line to nowhere keeps it that way.
    if (fromNode === undefined) continue;
    const onPath = node.onPath && fromNode.onPath;
    edges.push({
      id: `${fromNode.id}->${node.id}`,
      from: { layer: fromNode.layer, rank: fromNode.rank },
      to: { layer: node.layer, rank: node.rank },
      onPath,
      dimmed: !onPath,
      alpha: onPath ? TREE_ALPHA.onPath : TREE_ALPHA.offPathEdge,
      lineage: node.onLineage && fromNode.onLineage,
      x1: fromNode.x,
      y1: fromNode.y,
      x2: node.x,
      y2: node.y,
    });
  }

  const columns: TreeColumn[] = [];
  for (let layer = from; layer < from + drawnColumns; layer += 1) {
    const piece = pieceAt(options.sequence, layer);
    columns.push({
      layer,
      piece,
      x: xOf(layer),
      count: perLayerRecords[layer],
      drawn: perLayerDrawn[layer],
      label: `L${layer} ${piece}`,
    });
  }

  const stems = perLayerStems.reduce((sum, value) => sum + value, 0);
  const ticks = perLayerTicks.reduce((sum, value) => sum + value, 0);
  const chevrons = perLayerChevrons.reduce((sum, value) => sum + value, 0);
  const widest = perLayerRecords.length === 0 ? 0 : Math.max(...perLayerRecords);
  const label =
    bound >= widest
      ? `every state the trace holds — top ${bound} of the ${widest} kept per Layer · traced at band ${band}`
      : `top ${bound} of the ${widest} kept per Layer · traced at band ${band}`;

  return {
    bound,
    band,
    layerCount: trace.layerCount,
    records: trace.records.length,
    nodes,
    edges,
    columns,
    window: { from, count: drawnColumns, total },
    selection,
    lineage,
    counts: {
      perLayerDrawn,
      perLayerRecords,
      perLayerTicks,
      perLayerChevrons,
      perLayerStems,
      records: trace.records.length,
    },
    orderViolation,
    layout,
    label,
    caption: `the tree · ${plural(nodes.length, "state")} over ${plural(drawnColumns, "Layer")} · ${plural(
      edges.length,
      "edge",
    )}, ${plural(stems, "stem")}, ${plural(ticks, "tick")}, ${plural(chevrons, "chevron")}`,
  };
}

// ── the display list: what the canvas draws, in order ─────────────────────

export type TreeMarkKind = "state" | "tick" | "stem" | "chevron" | "edge";

export interface TreeMark {
  kind: TreeMarkKind;
  /** The state's id (`7:12`), or the edge's (`6:3->7:12`). */
  id: string;
  layer: number;
  rank: number;
  x: number;
  y: number;
  alpha: number;
  onPath: boolean;
  dimmed: boolean;
  selected: boolean;
  /** Edges only: the two states it joins. */
  from?: TreeRef;
  to?: TreeRef;
  /** Stems only: the parent the drawing had to drop. */
  orphan?: TreeOrphan;
  /** The state is on the winning lineage, so the mark is amber. */
  lineage: boolean;
  /** A mark's words, for a tooltip. */
  words: string;
}

/**
 * The model as a flat list of draw commands — edges first (under the marks),
 * then one primary mark per state, then its stem or chevron.
 *
 * The canvas is a consumer of this list and nothing else, which is what lets the
 * test assert the picture without a canvas: node count, tick count, the dim.
 */
export function treeDisplayList(model: TreeModel): TreeMark[] {
  const marks: TreeMark[] = [];
  for (const edge of model.edges) {
    marks.push({
      kind: "edge",
      id: edge.id,
      layer: edge.to.layer,
      rank: edge.to.rank,
      x: (edge.x1 + edge.x2) / 2,
      y: (edge.y1 + edge.y2) / 2,
      alpha: edge.alpha,
      onPath: edge.onPath,
      dimmed: edge.dimmed,
      selected: false,
      from: edge.from,
      to: edge.to,
      lineage: edge.lineage,
      words: `layer ${edge.to.layer} ← layer ${edge.from.layer} rank ${edge.from.rank}`,
    });
  }
  for (const node of model.nodes) {
    const onPath = node.onPath;
    marks.push({
      kind: node.mark,
      id: node.id,
      layer: node.layer,
      rank: node.rank,
      x: node.x,
      y: node.y,
      alpha: onPath ? TREE_ALPHA.onPath : TREE_ALPHA.offPathMark,
      onPath,
      dimmed: node.dimmed,
      selected: node.selected,
      lineage: node.onLineage,
      words: node.tooltip,
    });
    if (node.orphan !== null) {
      marks.push({
        kind: "stem",
        id: node.id,
        layer: node.layer,
        rank: node.rank,
        x: node.x,
        y: node.y,
        alpha: onPath ? TREE_ALPHA.onPath : TREE_ALPHA.offPathMark,
        onPath,
        dimmed: node.dimmed,
        selected: node.selected,
        orphan: node.orphan,
        lineage: node.onLineage,
        words: node.tooltip,
      });
    }
    if (node.cut) {
      marks.push({
        kind: "chevron",
        id: node.id,
        layer: node.layer,
        rank: node.rank,
        x: node.x,
        y: node.y,
        alpha: onPath ? TREE_ALPHA.onPath : TREE_ALPHA.offPathMark,
        onPath,
        dimmed: node.dimmed,
        selected: node.selected,
        lineage: node.onLineage,
        words: node.tooltip,
      });
    }
  }
  return marks;
}

/** The state nearest to a click, within the hit radius, or `null`. */
export function pickState(model: TreeModel, x: number, y: number): TreeRef | null {
  const radius = model.layout.hitRadius;
  let best: TreeNode | null = null;
  let bestDistance = Number.POSITIVE_INFINITY;
  for (const node of model.nodes) {
    const distance = Math.hypot(node.x - x, node.y - y);
    if (distance > radius) continue;
    if (distance < bestDistance) {
      best = node;
      bestDistance = distance;
    }
  }
  return best === null ? null : { layer: best.layer, rank: best.rank };
}

// ── the selected state's card ──────────────────────────────────────────────

/** Where a selected state left the winning lineage, when it is not on it. */
export interface TreeDivergence {
  /** The last Layer where the two paths agree. */
  layer: number;
  /** The rank this path took there. */
  rank: number;
  /** The rank the lineage took there. */
  lineageRank: number;
}

export interface TreeCard {
  ref: TreeRef;
  layer: number;
  rank: number;
  piece: string;
  score: number;
  lines: number;
  kidsInTrace: number;
  kidsInWindow: number;
  onLineage: boolean;
  /** The Board, when something can supply it; `null` when the trace cannot. */
  board: ArrayLike<number> | null;
  /** Why there is no Board, in words — never a blank rectangle. */
  boardNote: string | null;
  /** The record's numbers, in words and tabular numerals. */
  facts: string;
  divergence: TreeDivergence | null;
}

/**
 * The card under the columns for the selected state.
 *
 * §4.4's record is five `i32` and the ABI has no board accessor, so a state's Board
 * cannot be recovered from the trace: `boardAt` is the page's answer, and it can
 * only answer for the Boards it already holds (the root, and the played chain the
 * playback reconstructed). When it cannot, the card says so — "never a blank
 * rectangle and never a plausible-looking wrong board" (§2.1 item 1).
 */
export function stateCard(
  model: TreeModel,
  ref: TreeRef,
  options: { boardAt?: (layer: number, rank: number) => ArrayLike<number> | null } = {},
): TreeCard {
  const node = model.nodes.find((candidate) => candidate.layer === ref.layer && candidate.rank === ref.rank);
  const board = options.boardAt?.(ref.layer, ref.rank) ?? null;
  const lineageRef = model.lineage.find((point) => point.layer === ref.layer);
  const onLineage = lineageRef !== undefined && lineageRef.rank === ref.rank;

  const layer = ref.layer;
  const rank = ref.rank;
  const piece = node?.piece ?? (layer === 0 ? "root" : "?");
  const score = node?.score ?? 0;
  const lines = node?.lines ?? 0;
  const kidsInTrace = node?.kidsInTrace ?? 0;
  const kidsInWindow = node?.kidsInWindow ?? 0;
  const total = model.counts.perLayerRecords[layer] ?? 0;

  const facts = [
    `layer ${layer} (${piece})`,
    `rank ${rank} of ${total}`,
    `score ${score}`,
    `${lines} lines cleared`,
    node?.endCap === true
      ? "the last Layer"
      : `${plural(kidsInTrace, "child")} (${kidsInWindow} inside the top ${model.bound})`,
    onLineage ? "on the lineage that ends as the best final state" : "not on the lineage",
  ].join(" · ");

  return {
    ref: { layer, rank },
    layer,
    rank,
    piece,
    score,
    lines,
    kidsInTrace,
    kidsInWindow,
    onLineage,
    board,
    boardNote: board === null ? "this state's Board is not recorded in the trace" : null,
    facts,
    divergence: onLineage ? null : divergenceFromLineage(model, ref),
  };
}

/** The last Layer where this path and the winning lineage agree. */
function divergenceFromLineage(model: TreeModel, ref: TreeRef): TreeDivergence | null {
  const lineageByLayer = new Map(model.lineage.map((point) => [point.layer, point.rank]));
  // The path's own ranks per Layer, when the drawing holds them.
  const pathRanks = new Map<number, number>();
  let layer = ref.layer;
  let rank = ref.rank;
  for (;;) {
    pathRanks.set(layer, rank);
    if (layer === 0) break;
    const node = model.nodes.find((candidate) => candidate.layer === layer && candidate.rank === rank);
    if (node === undefined || node.parent === null) break;
    layer = node.parent.layer;
    rank = node.parent.rank;
  }
  let meet: TreeDivergence | null = null;
  for (const [at, pathRank] of pathRanks) {
    const lineageRank = lineageByLayer.get(at);
    if (lineageRank === undefined) continue;
    if (lineageRank !== pathRank) break;
    meet = { layer: at, rank: pathRank, lineageRank };
  }
  return meet;
}

/**
 * The state card's Board size: `cell = clamp(8, floor(innerWidth / 10), 14)`.
 *
 * At a 614-px inspector the cell is 14 px, so the Board is 140 × 280 and a one-cell
 * difference between neighbouring columns is a visible 14-px step — which is what
 * "column heights are legible" means. Below an 8-px cell the fallback ladder
 * shrinks the tree, never the card.
 */
export function stateCardLayout(inspectorWidth: number): { cell: number; width: number; height: number } {
  const inner = Math.max(80, Math.floor(inspectorWidth) - 48);
  const cell = Math.max(8, Math.min(14, Math.floor(inner / 10)));
  return { cell, width: cell * 10, height: cell * 20 };
}

// The decoder and the two sentinels travel with the model, so a consumer of the
// tree does not have to know the file split.
export { PARENT_NONE, PARENT_UNRECORDED };
