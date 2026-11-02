// The figure table: every number on the screen, its words label, its unit and
// where it came from.
//
// §2.1.2 is one sentence — "a tooltip on every control, and every figure labelled in
// words. A number on screen with no explanation is a defect" — and this file is that
// sentence as data. Every value the page displays is created through
// `createFigure(id)`, which stamps the element with
//
//   * `data-figure` — the id, so the headless check can count figures and prove a
//     regression that drops thirty of them;
//   * `data-figure-label` — the words, which must appear beside the numeral;
//   * `data-figure-source` — **where the number came from**, from the eight words
//     below, so "this figure is the engine's" is checkable and "this figure is a
//     placeholder" is visible;
//   * `data-figure-kind` — `number` (a numeral with a unit), `word` (an outcome, a
//     piece letter) or `picture` (a canvas, a plot, a bar row).
//
// Three rules the registry keeps, all of them from notes/14 §2:
//
// 1. **A figure the ABI cannot answer does not exist.** The mockup's per-Layer
//    "expanded"/"dedup" counters (`34 × min(...)`) are absent: §4.2 counts
//    evaluations per *Solve* and §4.4 records no candidate count, so a per-Layer
//    expansion figure would be a number no engine produced. `c14` is deliberately
//    not here, and `web/test/shell.test.mjs` asserts its absence.
// 2. **A placeholder says so in words.** The Run panel's rows carry
//    `not measured in this tab yet`, not a dash: a dash reads as "the region failed
//    to load" (notes/14 §8.1 A12).
// 3. **Provenance is one of eight words.** `solve` (the packed result), `trace`
//    (§4.4's records), `replay` (recomputed from `board0` + the Move list),
//    `dataset` (a committed artifact), `reference` (a committed Reference run),
//    `url` (the query string), `measured` (`performance.now()`), `control` (the value
//    the visitor set on a control).
//
// What this table cannot do is make a label *true*. That is the ticket's judged half
// (notes/14 §8.2), and it is why the report names which parts were read by eye.

export const FIGURE_SOURCES = [
  "solve",
  "trace",
  "replay",
  "dataset",
  "reference",
  "url",
  "measured",
  "control",
] as const;

export type FigureSource = (typeof FIGURE_SOURCES)[number];

export type FigureKind = "number" | "word" | "picture";

export interface FigureSpec {
  /** The words beside the numeral — the glossary's where it has one. */
  readonly label: string;
  /** The unit, when the figure is a numeral. A unit is part of the label, not the number. */
  readonly unit?: string;
  /** Where the number comes from. */
  readonly source: FigureSource;
  readonly kind: FigureKind;
  /** The whole explanation, in one sentence a reader can act on. */
  readonly tip: string;
}

/** Read it as a table: `id: spec(label, unit, source, kind, tip)`. */
function spec(label: string, unit: string, source: FigureSource, kind: FigureKind, tip: string): FigureSpec {
  return { label, ...(kind === "number" ? { unit } : {}), source, kind, tip };
}

/**
 * The ticket's table, id for id (notes/14 §2.1–§2.6).
 *
 * Ids are the notes' own (`t`oolbar, `e`ngine, `w`eights, `r`un, `c`entre, `m`ove,
 * `i`nspector, `s`tatus), because a reviewer with the notes open should be able to
 * find a row on the page by its name.
 */
export const FIGURES = {
  // ── toolbar ────────────────────────────────────────────────────────────────
  t1: spec("Seed", "integer", "url", "number",
    "Seed — the integer that fixes this game's starting Board and its Sequence. The same seed is always the same game, up to the height cap."),
  t2: spec("Goal", "Lines", "url", "number",
    "Goal — how many Lines this Solve must clear to count as a win. Changing it re-solves."),
  t3: spec("Pieces", "pieces", "url", "number",
    "Pieces — how many pieces the Sequence holds. One Piece is one Layer, so this is also the longest the game can run."),
  t4: spec("Beam width", "states per Layer", "dataset", "number",
    "Beam width — how many states survive each Layer. Narrower is cheaper and weaker. It opens at the shipped default from the measured width curve; the slider runs to the Width cap, where searching wider stops fitting its budget."),
  t5: spec("Lines cleared", "Lines", "solve", "word",
    "Lines cleared — the full rows this Solve removed, against the Goal. '8 of 8' means the Goal was met; a loss reads '6 of 8' and is an outcome, not an error."),
  t6: spec("elapsed", "ms", "measured", "number",
    "elapsed — wall-clock time this Solve took in this tab, measured around the engine call. It is this browser's number, not the Reference run's, and the two are not comparable without the label that says so."),
  t7: spec("outcome", "", "solve", "word",
    "outcome — whether the search cleared the Goal: solving, solved, or lost. A loss is an outcome and names the Lines it did reach; only a non-zero status is an error."),

  // ── rail: engine ───────────────────────────────────────────────────────────
  e1: spec("Beam width", "states per Layer", "url", "number",
    "Beam width — the same setting the Playground's slider carries: how many states survive each Layer. Separate from the tree's bound: a wider beam makes the band richer and the tree no more legible."),
  w1: spec("Landing height ×", "Q16", "dataset", "number",
    "Landing height — the height of the placed piece's centre of mass above the floor. Weight × Feature is what the evaluator charges the position for it."),
  w2: spec("Eroded piece cells ×", "Q16", "dataset", "number",
    "Eroded piece cells — rows this Placement completed times the piece cells those rows removed. The one positive Weight in the shipped table."),
  w3: spec("Row transitions ×", "Q16", "dataset", "number",
    "Row transitions — horizontal filled-to-empty edges per row, walls included. A jagged row is charged for being jagged."),
  w4: spec("Column transitions ×", "Q16", "dataset", "number",
    "Column transitions — vertical filled-to-empty edges per column. Holes and overhangs both show up here."),
  w5: spec("Holes ×", "Q16", "dataset", "number",
    "Holes — empty cells with a filled cell somewhere above them. The heaviest penalty after the height row, at −4.00."),
  w6: spec("Cumulative wells ×", "Q16", "dataset", "number",
    "Cumulative wells — the triangular sum of every well's depth: 1 + 2 + … + depth, for each well between two filled columns."),
  w7: spec("Hole depth ×", "Q16", "dataset", "number",
    "Hole depth — filled cells above each hole. Dellacherie's published function does not use it, so its Weight is zero and the row shows that rather than hiding it."),
  w8: spec("Rows with holes ×", "Q16", "dataset", "number",
    "Rows with holes — rows carrying at least one hole. Also absent from Dellacherie's function: a zero Weight, shown."),

  // ── rail: run ──────────────────────────────────────────────────────────────
  r1: spec("Win rate", "% of games", "reference", "number",
    "Win rate — the share of the test split's games this configuration won. 'yours' is measured in this tab; the reference column names the machine that produced it."),
  r2: spec("Games won", "games", "reference", "number",
    "Games won — how many of the test split's games cleared the Goal."),
  r3: spec("Mean Lines on failure", "Lines", "reference", "number",
    "Mean Lines on failure — on the games that were lost, the average Lines cleared. Losses only, so a weak search cannot flatter itself with wins."),
  r4: spec("Median Lines on failure", "Lines", "reference", "number",
    "Median Lines on failure — the same set, the middle game. Losses only."),
  r5: spec("Evaluations per Solve", "Evaluations", "reference", "number",
    "Evaluations per Solve — how many placements were scored per game: the median game and the 90th-percentile game. One Evaluation is one scored Placement, not a state and not a Layer."),
  r6: spec("ms per Solve", "ms", "reference", "number",
    "ms per Solve — wall-clock time per game, on the machine named under this table. Not this browser's number."),
  r7: spec("Total", "ms", "reference", "number",
    "Total — the whole Run's wall clock, both configurations together."),
  r8: spec("Reference run", "", "reference", "word",
    "Reference run — the machine, runtime and date every figure in the reference column came from: engine commit, wasm hash and build flags as well as CPU and OS. A number with no such label does not go on this page."),
  r9: spec("Run progress", "games", "measured", "number",
    "Run progress — games finished of the test split's total. A Run stops immediately with the stop control; nothing is left running behind the page."),

  // ── centre: readout ────────────────────────────────────────────────────────
  c1: spec("Layer", "Layers", "trace", "word",
    "Layer — the beam stage reached after placing that Piece of the Sequence. One Layer per Piece, and Dedup happens inside one Layer, never across Layers."),
  c2: spec("Goal", "Lines", "solve", "number",
    "Goal — how many Lines this Solve must clear to count as a win."),
  c5: spec("Position score", "score", "trace", "number",
    "Position score — the sum of the weighted Features for this Board: the number the search ranks states by. Larger is better; it is a fixed-point integer, not a percentage, and the decimal on screen is a rendering of it."),
  c6: spec("Column profile", "rows per column", "replay", "picture",
    "Column profile — each column's height in rows, with the holes its stack hides. Every bar is the Board at the Move on screen, recomputed from the engine's starting Board and Move list."),
  c7: spec("Placement", "piece", "replay", "word",
    "Placement — one legal rest of one Piece: its rotation and its column. It is the unit the beam expands, and one Placement becomes one Move when it survives into the solution."),
  c8: spec("Column", "0–9", "replay", "number",
    "Column — which of the ten columns the Piece was dropped into, 0 on the left."),
  c9: spec("Row", "0–19", "replay", "number",
    "Row — the row the Piece came to rest at, 0 at the top and 19 on the floor."),
  c10: spec("Clears", "rows this Placement", "replay", "number",
    "Clears — full rows this one Placement completed, before the stack above them falls. Not the cumulative Lines cleared since the game started."),
  c11: spec("Lines cleared", "Lines", "replay", "number",
    "Lines cleared — cumulative full rows removed since the game started, up to the Move on screen. Not the rows this Placement removed."),
  c12: spec("Next in Sequence", "pieces", "replay", "picture",
    "Next in Sequence — the pieces that follow the Move on screen, drawn from the Sequence the Generator fixed before the search started, from a 7-bag."),
  c13: spec("States kept", "states", "trace", "number",
    "States kept — how many states survived this Layer's trim and Dedup. The band is exactly this wide, because the band is the beam."),
  c15: spec("Best score", "score", "trace", "number",
    "Best score — the best score any state kept in this Layer reached."),
  c16: spec("States at the best score", "states", "trace", "number",
    "States at the best score — how many of this Layer's states share it. They are ordered by insertion after the score, so this is how wide the top of the band is."),
  c17: spec("Score spread", "score", "trace", "number",
    "Score spread — the gap between the best and worst score kept in this Layer. It is why the band's rows are score-positioned and not rank-positioned: per rank the spread is about a tenth of a point."),
  c18: spec("Score across Layers", "score", "trace", "picture",
    "Score across Layers — the best score kept at each Layer, left to right, and the range they span. The dot is the Layer on screen."),

  // ── centre: move strip and transport ──────────────────────────────────────
  m1: spec("Move strip", "cells", "replay", "picture",
    "Move strip — one cell per Move of the solution, and the seek bar: drag or click to any Move. A cell with an amber underline is a Move that completed at least one row."),
  m2: spec("Move", "Moves", "replay", "number",
    "Move — which Move of the solution is on screen. Move 0 is the starting Board, before anything has been placed."),
  m4: spec("Playback speed", "Moves per second", "control", "number",
    "Playback speed — continuous, not four presets: 0.25 to 40 Moves a second on a log scale. The time per Move is its reciprocal, and it is a preference, so it survives a new Solve."),

  // ── inspector ─────────────────────────────────────────────────────────────
  i1: spec("Score axis", "score", "trace", "picture",
    "Score — the evaluator's charge for that Board, which is what places every mark in a row. The axis is shared by every Layer, so one row is comparable with another."),
  i2: spec("Band", "states", "trace", "picture",
    "The band — one row per Layer, states placed by score, exactly as many states per row as survived the Layer, because the band is the beam. Traced by the engine, not recomputed here."),
  i3: spec("Winning lineage", "", "trace", "picture",
    "Winning lineage — the chain of states that ends as the best final state. Its rank wanders (it can be 175th in one Layer and first in the next), so this line is the answer, not the leader at each row."),
  i4: spec("Score tier", "", "trace", "picture",
    "Score tier — the shade is the state's score within its Layer's spread: the worst third, the middle, the best 18 per cent. Amber is reserved for the lineage and is never a tier."),
  i5: spec("Best score in Layer", "score", "trace", "number",
    "Best score in Layer — the best score kept in the row's Layer, at the end of the row."),
  i6: spec("Tree bound", "states per Layer", "control", "number",
    "Tree bound — how many of each Layer's best states the tree draws. It does not change the beam and it does not change the search: the band above is the beam."),
  i7: spec("Nodes", "nodes", "trace", "number",
    "Nodes — states inside the bound, drawn. Ticks are states with no child in the trace; stems are states whose parent fell outside the bound."),
  i8: spec("Layer window", "Layers", "trace", "word",
    "Layer window — at this game's Layer count the tree shows a window of Layers, centred on the selection. The band above shows where the window sits."),
  i9: spec("Per-layer table", "", "trace", "picture",
    "Per-layer table — one row per Layer: the Piece it placed, the best score it kept, the Lines cleared to reach it, and how many states survived it. Every column is the trace's."),
  i10: spec("Log", "ms since load", "measured", "picture",
    "Log — what happened, when, in order: worker boot, the ABI check, each solve call, its outcome, and any error. The times are milliseconds since this page loaded."),
  i11: spec("Selected state", "score", "trace", "word",
    "Selected state — the Board a chosen state had, reconstructed from the Move list, with its score and its Lines cleared. Nothing is selected until a state is clicked."),

  // ── status bar ────────────────────────────────────────────────────────────
  s1: spec("Evaluations", "Evaluations", "solve", "number",
    "Evaluations — placements the search scored, for this Solve. One Evaluation is one scored Placement: it is not a state and not a Layer, and the engine reports one count per Solve, so it does not rise during playback."),
  s2: spec("Layers", "Layers", "trace", "word",
    "Layers — the Layer on screen, of the Layers this game's trace holds. One Layer per Piece of the Sequence; a lost game keeps the Layers the search reached."),
  s3: spec("Lines cleared", "Lines", "solve", "word",
    "Lines cleared — cumulative full rows removed since the game started, against the Goal. A win reads '8 of 8'."),
  s4: spec("elapsed", "ms", "measured", "number",
    "elapsed — this page's own clock, measured around the engine call in this tab. It is not the engine's number (the packed result carries no time) and it is not comparable to the Reference run's ms per Solve."),
  s5: spec("Keyboard map", "", "control", "picture",
    "Keyboard map — every key the page takes. Space plays and pauses; the arrows step; Home and End jump; minus and equals change the speed; R restarts; E opens the explainer; ? opens the whole list. A focused field keeps its own keys."),
} as const satisfies Record<string, FigureSpec>;

export type FigureId = keyof typeof FIGURES;

/**
 * The ids the page must show, as a checklist a reviewer can read against the notes'
 * table. `FIGURES` may hold more than this; it may not hold less.
 *
 * `c14` — the mockup's per-Layer "states expanded" — is absent on purpose: §4.2 counts
 * evaluations per Solve and §4.4 records no candidate count, so the figure has no
 * source. It is deleted rather than marked "not measured", because a placeholder for
 * a number that could never exist is furniture.
 */
export const REQUIRED_FIGURES: readonly FigureId[] = [
  "t1", "t2", "t3", "t4", "t5", "t6", "t7",
  "e1", "w1", "w2", "w3", "w4", "w5", "w6", "w7", "w8",
  "r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8", "r9",
  "c1", "c2", "c5", "c6", "c7", "c8", "c9", "c10", "c11", "c12", "c13", "c15", "c16", "c17", "c18",
  "m1", "m2", "m4",
  "i1", "i2", "i3", "i4", "i5", "i6", "i7", "i8", "i9", "i10", "i11",
  "s1", "s2", "s3", "s4", "s5",
];

/** What an empty slot says before its first Solve. Never a dash (notes/14 §8.1 A12). */
export const NOT_MEASURED = "not measured in this tab yet";
export const AWAITING_SOLVE = "awaiting the first Solve";

export interface FigureView {
  readonly id: FigureId;
  readonly el: HTMLElement;
  readonly num: HTMLElement;
  readonly unit: HTMLElement;
  /** Put a value in the slot. `unit` overrides the registry's unit for one update. */
  set(value: string | number, unit?: string): void;
  /** The words label, as rendered. */
  label(): string;
}

/**
 * One figure element, stamped with the four attributes the checks read.
 *
 * The numeral and the unit are separate elements so the numeral stays tabular and
 * right-aligned in a cell sized by the widest value its unit allows — a number that
 * reflows the layout as it changes is the defect the tabular rule exists to prevent
 * (notes/14 §4.2).
 */
export function createFigure(id: FigureId, initial?: string): FigureView {
  const config = FIGURES[id];
  const el = document.createElement("span");
  el.className = "fig";
  el.dataset.figure = id;
  el.dataset.figureLabel = config.label;
  el.dataset.figureSource = config.source;
  el.dataset.figureKind = config.kind;
  el.dataset.tip = config.tip;

  const labelEl = document.createElement("span");
  labelEl.className = "flabel";
  labelEl.textContent = config.label;

  const num = document.createElement("b");
  num.className = "num";
  const unit = document.createElement("i");
  unit.className = "unit";
  if (config.unit) unit.textContent = config.unit;

  num.textContent = initial ?? (config.kind === "number" ? "—" : "…");
  el.append(labelEl, num, unit);

  const view: FigureView = {
    id,
    el,
    num,
    unit,
    set(value, nextUnit) {
      num.textContent = String(value);
      if (nextUnit !== undefined) unit.textContent = nextUnit;
    },
    label: () => config.label,
  };
  return view;
}

/** A figure's words and provenance, without building an element (for a static row). */
export function figureSpec(id: FigureId): FigureSpec {
  return FIGURES[id];
}

/**
 * Adopt a figure the markup already carries — the toolbar's four fields are static
 * so the first paint has words in it before any script runs, and this is how the page
 * gets a handle on them without rebuilding them.
 */
export function adoptFigure(id: FigureId, root: ParentNode = document): FigureView | null {
  const el = root.querySelector<HTMLElement>(`[data-figure="${id}"]`);
  const num = el?.querySelector<HTMLElement>(".num");
  const unit = el?.querySelector<HTMLElement>(".unit");
  if (!el || !num || !unit) return null;
  return {
    id,
    el,
    num,
    unit,
    set(value, nextUnit) {
      num.textContent = String(value);
      if (nextUnit !== undefined) unit.textContent = nextUnit;
    },
    label: () => FIGURES[id].label,
  };
}
