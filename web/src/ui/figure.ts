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
  // ── rail: engine ───────────────────────────────────────────────────────────
  e1: spec("Beam width", "states per layer", "url", "number",
    "The Playground's slider: how many states survive each layer. Narrower is cheaper and weaker."),
  w1: spec("Landing height ×", "Q16", "dataset", "number",
    "Charges for how high the placed piece's centre of mass ends up."),
  w2: spec("Eroded piece cells ×", "Q16", "dataset", "number",
    "Rows the placement completed, times the piece cells those rows removed. The one positive weight."),
  w3: spec("Row transitions ×", "Q16", "dataset", "number",
    "Charges for filled-to-empty edges along each row: a jagged row costs."),
  w4: spec("Column transitions ×", "Q16", "dataset", "number",
    "Charges for filled-to-empty edges down each column: holes and overhangs show up here."),
  w5: spec("Holes ×", "Q16", "dataset", "number",
    "Charges for empty cells with a filled cell somewhere above them. A heavy penalty."),
  w6: spec("Cumulative wells ×", "Q16", "dataset", "number",
    "Sum of every well's depth between two filled columns; deep wells cost quadratically."),
  w7: spec("Hole depth ×", "Q16", "dataset", "number",
    "Charges for filled cells above each hole. Weight is zero: shipped that way, shown that way."),
  w8: spec("Rows with holes ×", "Q16", "dataset", "number",
    "Charges for each row carrying at least one hole. Also zero weight, shown."),

  // ── rail: run ────────────────────────────────────────────────────────────────
  r1: spec("Win rate", "% of games", "reference", "number",
    "The share of games this run won. Your column and the reference column name their machines below the table."),
  r2: spec("Games won", "games", "reference", "number",
    "How many games of the run cleared the goal."),
  r3: spec("Mean lines on failure", "lines", "reference", "number",
    "On lost games, the average lines cleared. Losses only, so weak searches cannot hide behind wins."),
  r4: spec("Median lines on failure", "lines", "reference", "number",
    "The same lost games, the middle one."),
  r5: spec("Evaluations per solve", "evaluations", "reference", "number",
    "Placements scored per game, median and p90. One evaluation is one scored placement."),
  r6: spec("ms per solve", "ms", "reference", "number",
    "Wall-clock time per game, median and p90. Not comparable across machines, so each column names its clock."),
  r7: spec("Total", "ms", "reference", "number",
    "The whole run's time in the clock its column names. Your column's total includes the worker message hops."),
  r8: spec("Reference run", "", "reference", "word",
    "The machine, runtime and date behind the reference column, committed with the Dataset."),
  r9: spec("Run progress", "games", "measured", "number",
    "Games finished of the run's total. Stop ends workers immediately; nothing keeps running behind the page."),

  // ── centre: readout ────────────────────────────────────────────────────────
  c1: spec("Layer", "", "trace", "word",
    "Which layer of the search is on screen. One layer per piece placed."),
  c5: spec("Position score", "score", "trace", "number",
    "The evaluator's total charge for this board: the number the search ranks states by. Larger is better."),
  c6: spec("Column profile", "rows per column", "replay", "picture",
    "Each column's height in rows, with its hidden holes marked. It redraws for the board on screen."),
  c7: spec("Placement", "piece", "replay", "word",
    "The piece and rotation of the move on screen."),
  c8: spec("Column", "0–9", "replay", "number",
    "Which of the ten columns the piece was dropped into."),
  c9: spec("Row", "0–19", "replay", "number",
    "The row the piece came to rest at."),
  c10: spec("Clears", "rows", "replay", "number",
    "Full rows this one placement completed."),
  c11: spec("Lines cleared", "lines", "replay", "number",
    "Cumulative lines removed since the game started, at the move on screen."),
  c12: spec("Next pieces", "pieces", "replay", "picture",
    "The pieces that follow the move on screen."),

  // ── centre: move strip and transport ─────────────────────────
  m1: spec("Move strip", "", "replay", "picture",
    "One cell per move of the solution, doubled as the seek bar: click or drag to any move."),
  m2: spec("Move", "of the solution", "replay", "number",
    "Which move of the solution is on screen. Move 0 is the starting board."),
  m4: spec("Playback speed", "moves/s", "control", "number",
    "Playback speed, 0.25 to 40 moves a second. It is a preference, so it survives a new solve."),

  // ── inspector ──────────────────────────
  i1: spec("Score axis", "", "trace", "picture",
    "States are placed by the evaluator's charge for their board. Rows share one axis."),
  i2: spec("Band", "states", "trace", "picture",
    "One row per layer, exactly as many states per row as the search kept. Traced, not recomputed."),
  i3: spec("Winning lineage", "", "trace", "picture",
    "The chain of states ending in the win. Its rank wanders per layer, so the line, not the row leader, is the answer."),
  i4: spec("Score tier", "", "trace", "picture",
    "Shade is the state's score within its layer: worst third, middle, best 18%."),
  i5: spec("Best score in layer", "score", "trace", "number",
    "The best score any state kept in the row's layer reached."),
  i6: spec("Tree bound", "states per layer", "control", "number",
    "How many of each layer's best states the tree draws. Display only; the search is unchanged."),
  i7: spec("Nodes", "nodes", "trace", "number",
    "States drawn inside the bound. Ticks have no child in the trace; stems are where a parent fell outside the bound."),
  i8: spec("Layer window", "", "trace", "word",
    "The tree shows a window of layers, centred on the selection."),
  i9: spec("Per-layer table", "", "trace", "picture",
    "One row per layer: piece placed, best score kept, lines cleared, states surviving."),
  i11: spec("Selected state", "", "trace", "word",
    "The board a chosen state had, reconstructed from the move list."),

  // ── status bar ───
  s1: spec("Evaluations", "per solve", "solve", "number",
    "Placements the search scored for this solve. One evaluation is one scored placement."),
  s5: spec("Keyboard map", "", "control", "picture",
    "The keys this page takes. ? shows the whole list."),
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
  "e1", "w1", "w2", "w3", "w4", "w5", "w6", "w7", "w8",
  "r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8", "r9",
  "c1", "c5", "c6", "c7", "c8", "c9", "c10", "c11", "c12",
  "m1", "m2", "m4",
  "i1", "i2", "i3", "i4", "i5", "i6", "i7", "i8", "i9", "i11",
  "s1", "s5",
];

/**
 * The figures whose words live in `index.html` as well as here: the first paint has to
 * say something before any script runs, so these two repeat the registry, and
 * `web/test/shell.test.mjs` fails if the repetition drifts.
 *
 * The list is a ceiling, not a note. Every other figure is built by `createFigure` at
 * boot, so a third entry is a decision to maintain a second copy. If a new static
 * figure is tempting, stamp its attributes at boot instead.
 */
export const STATIC_FIGURES = ["m4", "s5"] as const satisfies readonly FigureId[];

/** What an empty slot says before its first solve. It means the same on every run. */
export const NOT_MEASURED = "not measured yet";
export const AWAITING_SOLVE = "no solve yet";

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
