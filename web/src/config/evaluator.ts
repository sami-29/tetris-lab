// The shipped Evaluator, as the Engine panel shows it: eight Features, eight
// integer Weights, eight bounds.
//
// Three facts decide this file's shape.
//
// 1. **There is no ABI export for the Weights.** `solve` takes no evaluator
//    argument, the URL's keys are `seed, goal, pieces, cap, beam` (§2.2), and
//    `engine/src/evaluator.rs` says so in as many words: the loader can reach
//    `shipped()` and nothing else. So the page cannot *ask* what the weights are; it
//    carries the published table and shows it.
// 2. **A carried table drifts, so it is guarded.** `web/test/evaluator-weights.test.mjs`
//    parses `engine/src/evaluator.rs` and compares all eight Q16 integers with the
//    ones below, and checks that the shipped model is the one named here. Ticket
//    07/08's verdict (a CE-tuned table, or E0 winning the ablation) therefore fails a
//    test in the page rather than printing last week's numbers.
// 3. **A Weight is an integer, not a decimal.** It is Q16: the score is
//    `Σ weight[i] · feature[i] >> 16`, exactly, so native and wasm agree. The
//    decimal on screen is a *rendering* of that integer — `decimalOf` — which is why
//    the tooltip says which is the source of truth (§2.1.2, notes/14 E2).
//
// The bounds are the paper's own ranges (Thiery & Scherrer, Table 1, as transcribed
// in notes/05 §2.1). They are what the readout's bars are scaled against, so a bar
// reaching the end of its track means the Feature is at the top of what the board can
// physically produce — not "bigger than the other rows today".

/** `engine/src/evaluator.rs`: one Q16 unit. */
export const Q16 = 65536;

export interface FeatureSpec {
  /** The Rust struct's field name, so the drift test can match it by name. */
  readonly key: string;
  /** The words the panel prints. */
  readonly label: string;
  /** The paper's own words for the Feature (Table 1), verbatim. */
  readonly paper: string;
  /** The Weight, as the engine holds it. The decimal on screen is derived from this. */
  readonly weightQ16: number;
  /** `[min, max]` of the Feature, which is the scale a bar is drawn against. */
  readonly bound: readonly [number, number];
  /** The sentence beside the row: what the Feature measures, its unit and its bound. */
  readonly tip: string;
}

export interface EvaluatorSpec {
  /** The model the ABI's `shipped()` returns: `E1` (Dellacherie–Thiery). */
  readonly model: "E0" | "E1" | "E2";
  /** How the panel names it. */
  readonly name: string;
  /** Where every integer here comes from, for the panel's provenance line. */
  readonly source: string;
  readonly features: readonly FeatureSpec[];
}

/**
 * `E1_PUBLISHED`, field for field, weight for weight
 * (`engine/src/evaluator.rs`, "E1: the eight Features of Thiery & Scherrer at
 * published weights").
 *
 * Eight Features rather than the four the mockup sliders moved: the mockup invented
 * a holes weight of `3.20` against the engine's `−4.00`, which is the fabrication
 * ticket 14 exists to delete (notes/14 §2.7).
 */
export const SHIPPED_EVALUATOR: EvaluatorSpec = {
  model: "E1",
  name: "Dellacherie–Thiery, published weights",
  source: "engine/src/evaluator.rs · E1_PUBLISHED",
  features: [
    {
      key: "landing_height",
      label: "Landing height",
      paper: "Height where the last piece is added",
      weightQ16: -Q16,
      bound: [1, 20],
      tip: "Landing height × −1.00 of Q16 −65,536 — the height of the placed piece's centre of mass above the floor, 1 to 20 rows. Weight × Feature is what the evaluator charges the position for it.",
    },
    {
      key: "eroded_cells",
      label: "Eroded piece cells",
      paper: "(rows eliminated in the last move) × (cells of the last piece eliminated)",
      weightQ16: Q16,
      bound: [0, 16],
      tip: "Eroded piece cells × +1.00 of Q16 65,536 — rows this Placement completed times the piece cells those rows removed, 0 to 16. The one Feature that is positive: it pays for the placement that did the work.",
    },
    {
      key: "row_transitions",
      label: "Row transitions",
      paper: "Number of horizontal cell transitions",
      weightQ16: -Q16,
      bound: [0, 220],
      tip: "Row transitions × −1.00 of Q16 −65,536 — horizontal filled-to-empty edges per row, walls included, 0 to 220. A jagged row is charged for being jagged.",
    },
    {
      key: "col_transitions",
      label: "Column transitions",
      paper: "Number of vertical cell transitions",
      weightQ16: -Q16,
      bound: [0, 210],
      tip: "Column transitions × −1.00 of Q16 −65,536 — vertical filled-to-empty edges per column, 0 to 210. Holes and overhangs both show up here.",
    },
    {
      key: "holes",
      label: "Holes",
      paper: "Number of empty cells covered by a full cell",
      weightQ16: -4 * Q16,
      bound: [0, 200],
      tip: "Holes × −4.00 of Q16 −262,144 — empty cells with a filled cell somewhere above them, 0 to 200. The heaviest single penalty after the height row.",
    },
    {
      key: "wells",
      label: "Cumulative wells",
      paper: "Σ over wells of (1 + 2 + ··· + depth)",
      weightQ16: -Q16,
      bound: [0, 840],
      tip: "Cumulative wells × −1.00 of Q16 −65,536 — the triangular sum of every well's depth, 0 to 840. A well is a column gap with filled neighbours on both sides.",
    },
    {
      key: "hole_depth",
      label: "Hole depth",
      paper: "Number of full cells in the column above each hole",
      weightQ16: 0,
      bound: [0, 1000],
      tip: "Hole depth × 0.00 of Q16 0 — filled cells above each hole, 0 to 1000. Dellacherie's function does not use it (§4 of the paper), so its Weight is zero and it is listed to show that it is.",
    },
    {
      key: "rows_with_holes",
      label: "Rows with holes",
      paper: "Number of rows having at least one hole",
      weightQ16: 0,
      bound: [0, 20],
      tip: "Rows with holes × 0.00 of Q16 0 — rows carrying at least one hole, 0 to 20. Also absent from Dellacherie's function: a zero Weight, shown rather than hidden.",
    },
  ],
};

/** The decimal a Q16 integer renders as — `-1.00`, `+1.00`, `0.00`. */
export function decimalOf(weightQ16: number): string {
  const value = weightQ16 / Q16;
  if (value === 0) return "0.00";
  const fixed = Math.abs(value).toFixed(2);
  return `${value > 0 ? "+" : "-"}${fixed}`;
}

/** One Feature by its Rust field name. Throws rather than returning a hole. */
export function featureOf(key: string): FeatureSpec {
  const feature = SHIPPED_EVALUATOR.features.find((candidate) => candidate.key === key);
  if (!feature) throw new Error(`the shipped evaluator has no Feature called ${key}`);
  return feature;
}

/**
 * The weight's on-screen unit, in the glossary's words (§2.1.2's "with its unit").
 *
 * The integer is the unit because the integer is the contract: `Q16 · −65,536`. A
 * visitor who reads only the decimal still sees what it was divided from.
 */
export function weightUnit(feature: FeatureSpec): string {
  const integer = feature.weightQ16.toLocaleString("en-US");
  return `Q16 ${feature.weightQ16 > 0 ? "+" : ""}${integer}`;
}
