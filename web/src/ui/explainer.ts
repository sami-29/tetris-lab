// The Explainer: what the drawer says, in the words of the engine that shipped.
//
// The prose lives here rather than in `index.html` for one reason: every figure it
// quotes has to come from an artifact, and a static paragraph cannot be checked
// against one. So a section is a *lead* and a list of segments, and a segment is
// either words or a named fact:
//
//   * the **words** are ticket 16's text, written once (notes/16 §1.2), and they
//     contain no digit of their own — `test/explainer.test.mjs` fails on one;
//   * a **fact** is resolved here from `src/config/width.ts`,
//     `src/config/evaluator.ts` or `src/generated/engine-facts.ts`, and carries the
//     artifact path in its tooltip, so a visitor can point at the same number on
//     the page and re-check it there.
//
// The drawer's mechanics are ticket 14's and are not repeated here: it is a fixed
// panel that slides on `transform`, it opens from `How it works` and from `E`, and
// it cannot add a page scroll because it is out of flow in a page whose root is
// `overflow: hidden`. This module only fills `#explainer-body`.
//
// What the text deliberately does not say: anything about the application this page
// replaced, any second Evaluator architecture, any per-Layer Evaluation count (the
// ABI carries one count per Solve), and any quality figure. The drawer explains the
// mechanism; the Run and the Reference row carry the measurements.

import {
  DEFAULT_BEAM,
  DEFAULT_BEAM_BASIS,
  QUALITY_AXIS,
  WIDTH_CAP,
} from "../config/width.ts";
import { SHIPPED_EVALUATOR, decimalOf } from "../config/evaluator.ts";
import { ENGINE_FACTS } from "../generated/engine-facts.ts";

/** A figure the prose may quote. Every one resolves from an artifact. */
export type FactId =
  | "default-beam"
  | "width-cap"
  | "tail"
  | "evaluator-name"
  | "feature-count"
  | "dataset-configs"
  | "games"
  | "test-range"
  | "tune-range"
  | "reference-machine"
  | "reference-runtime"
  | "reference-date"
  | "reference-commit";

export type Segment = string | { readonly fact: FactId };

export interface Section {
  /** The run-in heading: the glossary's term, bold, inside the paragraph. */
  readonly lead: string;
  readonly body: readonly Segment[];
}

/** What named the default, as the width curve records it. */
export interface WidthStory {
  readonly defaultBeam: number;
  readonly widthCap: number;
  readonly basis: "cost" | "tie";
  readonly qualityAxis: "no-resolution" | "resolved";
}

const SHIPPED_STORY: WidthStory = {
  defaultBeam: DEFAULT_BEAM,
  widthCap: WIDTH_CAP,
  basis: DEFAULT_BEAM_BASIS,
  qualityAxis: QUALITY_AXIS,
};

/**
 * The sentence that says what named the default — selected, never patched.
 *
 * Three variants, chosen by the artifact's own fields (notes/16 §5.2): a measured
 * tie, a cost decision with a quality axis that could not separate the widths, and
 * the degenerate case where the default *is* the cap. The toolbar's tooltip reads
 * the same fields, so the two cannot tell different stories.
 */
export function beamStory(story: WidthStory): string[] {
  if (story.defaultBeam === story.widthCap) {
    return [
      "The default is the cap: the width curve was still climbing when the time budget stopped it, so the Width cap is also the slider's maximum.",
    ];
  }
  if (story.basis === "tie") {
    return [
      "The default is the narrowest width that ties the widest one a Solve can still afford; the slider runs to the Width cap",
    ];
  }
  return [
    story.qualityAxis === "resolved"
      ? "The widths tied on the ranking bed, so the default was named on cost; the slider runs to the Width cap"
      : "No measured width beat another, so the default was named on cost; the slider runs to the Width cap",
  ];
}

/** The beam paragraph, with the cap in its own slot. */
function beamParagraph(story: WidthStory): Segment[] {
  const parts: Segment[] = [
    "One Layer per piece of the Sequence. Each Layer expands every surviving state into every legal Placement, scores the Board, and keeps the best ",
    { fact: "default-beam" },
    ". That count is the Beam width: a wider beam keeps more alive and costs Evaluations in proportion. ",
    ...beamStory(story),
  ];
  if (story.defaultBeam !== story.widthCap) parts.push(", ", { fact: "width-cap" }, ".");
  return parts;
}

const REFERENCE_PENDING: string[] =
  ENGINE_FACTS.reference.status === "measured" && ENGINE_FACTS.reference.rows.length > 0
    ? []
    : [
        "A Reference run is the committed result of pushing the Dataset through the native harness on one machine, labelled with that machine, the runtime, the date and the engine commit. Its quality rows are held exact for an identical engine and Dataset fingerprint; only its timings may differ. No Reference row is committed yet, so this page quotes none.",
      ];

const REFERENCE_MEASURED: Segment[] = [
  "A Reference run is the committed result of pushing the Dataset through the native harness: ",
  { fact: "reference-machine" },
  ", ",
  { fact: "reference-runtime" },
  ", ",
  { fact: "reference-date" },
  ", engine commit ",
  { fact: "reference-commit" },
  ". Its quality rows are held exact for an identical engine and Dataset fingerprint; only its timings may differ.",
];

/**
 * The nine sections, in the order a visitor meets them.
 *
 * Run-in headings rather than `<h3>`: the drawer is a fixed panel with a fixed
 * amount of vertical room, and a heading block costs two lines each.
 */
export const EXPLAINER_SECTIONS: readonly Section[] = [
  {
    lead: "The Lab",
    body: [
      "The Lab solves one game, plays it back, and can solve the Dataset beside it. A game is a Board and a Sequence, both fixed by a Seed: the Generator turns the Seed into a starting Board and the pieces to come.",
    ],
  },
  { lead: "Beam width", body: beamParagraph(SHIPPED_STORY) },
  {
    lead: "Dedup",
    body: [
      "Two states in one Layer can hold the same Board. Dedup keeps one, the state with more Lines cleared, because the Board decides what follows. It never compares across Layers: the same Board at a later Layer is a different state.",
    ],
  },
  {
    lead: "Tail search",
    body: [
      "The last ",
      { fact: "tail" },
      " pieces are searched exactly. Over the final ",
      { fact: "tail" },
      " Layers the beam's approximation no longer pays for itself, so every legal Placement is expanded from every surviving state, and the first combination that reaches the Goal wins. It cannot recover a state the beam discarded earlier; it removes the approximation only where the depth is short enough to afford none. It is the one part of the search that can be switched off: Dedup and the No-suicide rule are unconditional.",
    ],
  },
  {
    lead: "No-suicide rule",
    body: [
      "A Placement that ends the game is only legal when no other Placement is legal. It never rescues a lost Board; it stops the search choosing a game-ending Placement while a legal one exists.",
    ],
  },
  {
    lead: "Evaluator",
    body: [
      "One Evaluator scores every resulting Board: ",
      { fact: "evaluator-name" },
      ", over ",
      { fact: "feature-count" },
      " Features. A Feature is an integer count of the Board; a Weight is a fixed-point multiplier, so a score is an integer and the native harness and this page agree exactly. That is what lets the Reference run sit beside a visitor's Run with no correction.",
    ],
  },
  {
    lead: "Dataset",
    body: [
      "The Dataset is the committed set of games every figure here is measured on: ",
      { fact: "dataset-configs" },
      " configurations, ",
      { fact: "games" },
      " games each, in two Splits. Seeds ",
      { fact: "test-range" },
      " are the test Split, the only games a Run and the Reference run touch; seeds ",
      { fact: "tune-range" },
      " are the tuning Split, which only tuning, ablations and the width curve use. A Dataset fingerprint identifies the games: change any game and it moves.",
    ],
  },
  {
    lead: "Reference run",
    body: REFERENCE_PENDING.length > 0 ? REFERENCE_PENDING : REFERENCE_MEASURED,
  },
  {
    lead: "Run",
    body: [
      "A Run solves the test Split of both configurations in this tab and reports per configuration: win rate, mean and median Lines cleared on the games it lost, Evaluations per Solve at the median and the ninetieth percentile, ms per Solve at the same points, and the total.",
    ],
  },
];

/** The leads alone, for a structural assertion and for the mount's order. */
export const EXPLAINER_LEADS: readonly string[] = EXPLAINER_SECTIONS.map((section) => section.lead);

const latestReference = ENGINE_FACTS.reference.rows[0];

/**
 * Every fact's rendered text and the artifact it came from.
 *
 * The tooltip is not decoration: ticket 16 requires every quoted figure to be
 * attributable, and "the page can be re-checked against it" means naming the field
 * a reader can open. The weight block's tooltips come from the Evaluator table
 * (`src/config/evaluator.ts`) and say the same thing beside the Engine panel's rows.
 */
export const FACT_TEXT: Record<FactId, string> = {
  "default-beam": String(DEFAULT_BEAM),
  "width-cap": String(WIDTH_CAP),
  tail: String(ENGINE_FACTS.tail.pieces),
  "evaluator-name": SHIPPED_EVALUATOR.name,
  "feature-count": String(SHIPPED_EVALUATOR.features.length),
  "dataset-configs": String(ENGINE_FACTS.dataset.configCount),
  games: ENGINE_FACTS.dataset.games,
  "test-range": ENGINE_FACTS.dataset.testRange,
  "tune-range": ENGINE_FACTS.dataset.tuneRange,
  "reference-machine": latestReference?.machine ?? "",
  "reference-runtime": latestReference?.runtime ?? "",
  "reference-date": latestReference?.date ?? "",
  "reference-commit": latestReference?.commit ?? "",
};

export const FACT_TIP: Record<FactId, string> = {
  "default-beam": `Beam width — ${DEFAULT_BEAM} states per Layer. data/measurements/width-curve.json (default_beam); the toolbar's field opens at it and the band holds that many states in every row.`,
  "width-cap": `The Width cap — ${WIDTH_CAP}. data/measurements/width-curve.json (width_cap); the width slider's maximum, fixed by the time budget rather than by taste.`,
  tail: `The Tail search's length — the last ${ENGINE_FACTS.tail.pieces} pieces. engine/src/search.rs (TAIL), the same constant the search reads.`,
  "evaluator-name": `The shipped Evaluator. engine/src/evaluator.rs (${ENGINE_FACTS.shipped.table}), chosen by data/measurements/ablation-ladder.json; the Engine panel shows the same table.`,
  "feature-count": `How many Features the shipped Evaluator scores. engine/src/evaluator.rs (${ENGINE_FACTS.shipped.table}); the Engine panel's Expert view lists every Weight.`,
  "dataset-configs": `Dataset configurations. data/dataset/*/manifest.json, one committed games file each.`,
  games: `Games per configuration. data/dataset/*/manifest.json (game_count); the Run progress counts the same set.`,
  "test-range": `The test Split's seeds. data/dataset/*/manifest.json (splits.test) — the only games a Run and a Reference run measure.`,
  "tune-range": `The tuning Split's seeds. data/dataset/*/manifest.json (splits.tuning) — read by tuning, ablations and the width curve, never by a Run.`,
  "reference-machine": `The machine the Reference row was measured on. data/dataset/<configuration>/reference.jsonl (the latest row).`,
  "reference-runtime": `The runtime the Reference row was measured on. data/dataset/<configuration>/reference.jsonl (the latest row).`,
  "reference-date": `When the Reference row was measured. data/dataset/<configuration>/reference.jsonl (the latest row).`,
  "reference-commit": `The engine commit the Reference row was measured at. data/dataset/<configuration>/reference.jsonl (the latest row).`,
};

/** The prose as a visitor reads it — facts resolved, headings included. */
export function explainerText(): string {
  const paragraphs = EXPLAINER_SECTIONS.map((section) => {
    const body = section.body
      .map((segment) => (typeof segment === "string" ? segment : FACT_TEXT[segment.fact]))
      .join("");
    return `${section.lead}. ${body}`;
  });
  return [...paragraphs, weightText()].join("\n\n");
}

/** Every literal part of the prose: the segments that are words, never facts. */
export function literalSegments(): string[] {
  return EXPLAINER_SECTIONS.flatMap((section) =>
    section.body.filter((segment): segment is string => typeof segment === "string"),
  );
}

/** One row of the Weight block: the Feature, its integer's rendering, and its tip. */
export interface WeightRow {
  readonly label: string;
  readonly value: string;
  readonly tip: string;
}

/** The shipped table, eight rows, in the engine's field order. */
export function weightRows(): WeightRow[] {
  return SHIPPED_EVALUATOR.features.map((feature) => ({
    label: feature.label,
    value: decimalOf(feature.weightQ16),
    tip: feature.tip,
  }));
}

/** The weight block as plain text, for the word scan and for the no-digit check. */
function weightText(): string {
  return weightRows()
    .map((row) => `${row.label} × ${row.value}`)
    .join(" · ");
}

/** Build one paragraph: a bold run-in lead, then words and fact spans. */
function paragraph(section: Section): HTMLParagraphElement {
  const p = document.createElement("p");
  const lead = document.createElement("b");
  lead.textContent = `${section.lead}.`;
  p.append(lead, " ");
  for (const segment of section.body) {
    if (typeof segment === "string") {
      p.append(segment);
      continue;
    }
    const span = document.createElement("span");
    span.className = "fig";
    span.textContent = FACT_TEXT[segment.fact];
    span.dataset.tip = FACT_TIP[segment.fact];
    p.append(span);
  }
  return p;
}

/** The Weight block: two columns of four, each pair labelled and explained. */
function weights(): HTMLParagraphElement {
  const block = document.createElement("p");
  block.className = "weights";
  for (const row of weightRows()) {
    const pair = document.createElement("span");
    pair.className = "wpair";
    pair.dataset.tip = row.tip;
    const name = document.createElement("span");
    name.className = "wname";
    name.textContent = row.label;
    const value = document.createElement("span");
    value.className = "num";
    value.textContent = `× ${row.value}`;
    pair.append(name, " ", value);
    block.append(pair);
  }
  return block;
}

/**
 * Fill the drawer's prose home.
 *
 * Idempotent: the keyboard sheet is a sibling (`#sheet`) and is never touched, and a
 * second call replaces the prose rather than doubling it.
 */
export function mountExplainer(root: HTMLElement): void {
  root.replaceChildren(...EXPLAINER_SECTIONS.map(paragraph), weights());
}
