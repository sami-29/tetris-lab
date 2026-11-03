// The one line that explains why the width field opens where it does.
//
// §2.2: "The toolbar's width field opens at `DEFAULT_BEAM`; the slider runs to the
// cap with a labelled tick at the default, and one tooltip line explains why they
// differ ('the narrowest width statistically as good as the widest we can afford').
// If the default *is* the cap, the tick merges into the slider's end and the tooltip
// says the timeout is what stopped it."
//
// The quoted sentence is the *pre-registered* case. What this build's artifact
// records is a different story, and the tooltip has to tell that one:
//
//   * `default_beam_basis: "cost"` — the default was named on cost, because
//   * `quality_axis: "no-resolution"` — no measured width beat another, so the curve
//     could not rank the widths by quality at all; and
//   * `cap_bound: "schedule"` — the sweep stopped where the pre-registered doubling
//     list ended, not at a timeout.
//
// So nothing here is typed in prose as a claim about the measurement: the sentence
// is composed from the artifact's own fields, and `web/test/width-tooltip.test.mjs`
// fails if it starts claiming a quality optimum the artifact does not have. A
// re-measurement changes three fields and this copy changes with them — which is the
// whole reason the numbers were put in an artifact (§9.5: "the published table, the
// URL default and the slider maximum therefore cannot drift").

import {
  CAP_BOUND,
  DEFAULT_BEAM,
  DEFAULT_BEAM_BASIS,
  QUALITY_AXIS,
  WIDTH_CAP,
} from "../config/width.ts";

/** What named the two numbers, as the width curve records it. */
export interface WidthStory {
  readonly defaultBeam: number;
  readonly widthCap: number;
  /** `default_beam_basis`: what the default was chosen on. */
  readonly basis: "cost" | "tie";
  /** `quality_axis`: whether the quality series could separate the widths. */
  readonly qualityAxis: "no-resolution" | "resolved";
  /** `cap_bound`: what ended the sweep. */
  readonly capBound: "budget" | "schedule";
}

/** The story this build's artifact tells, read at build time. */
export const SHIPPED_WIDTH_STORY: WidthStory = {
  defaultBeam: DEFAULT_BEAM,
  widthCap: WIDTH_CAP,
  basis: DEFAULT_BEAM_BASIS,
  qualityAxis: QUALITY_AXIS,
  capBound: CAP_BOUND,
};

/** What stopped the sweep, in the words the field can support. */
function whyItEnded(story: WidthStory): string {
  return story.capBound === "budget"
    ? "the analysis time budget stopped the width curve"
    : "the pre-registered doubling schedule ran out";
}

/**
 * The tooltip: one line, both numbers, and the reason they are different decisions.
 *
 * `default === cap` is the degenerate case §2.2 names — the tick merges into the
 * slider's end and there is no narrower width to point at, so the line says what
 * stopped it and stops.
 */
export function widthTooltip(story: WidthStory = SHIPPED_WIDTH_STORY): string {
  const cap = String(story.widthCap);
  const ended = whyItEnded(story);

  if (story.defaultBeam === story.widthCap) {
    return `The default is the cap (${cap}): ${ended}, so there is no second number to explain.`;
  }

  let named: string;
  if (story.basis === "tie") {
    named = `Default ${story.defaultBeam} is the narrowest width that wins as often as the widest we can afford`;
  } else {
    const why = story.qualityAxis === "resolved"
      ? "the widths tied on the ranking bed"
      : "no measured width beat another, so the widths could not be separated";
    named = `Default ${story.defaultBeam} was named on cost, not on a quality result: ${why}`;
  }

  return `${named}; cap ${cap} is where ${ended}, so the slider stops while a solve still fits its budget.`;
}
