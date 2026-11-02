// The two configurations the site publishes figures for, as buttons.
//
// §6 commits exactly two Dataset configurations and §9.6 quotes a Reference row per
// configuration, "all solves at `DEFAULT_BEAM`". A preset is therefore not a
// convenience: it is the pointer from a button on the page to a row in the
// published table, and it has to fill *five* values — the Playground's four plus
// the width the figures were measured at — or it could not reproduce anything.
//
// The four fixed values live here and are asserted against
// `data/dataset/<id>/manifest.json` by `web/test/presets.test.mjs`, so a button
// cannot quote a configuration the committed Dataset does not have. The fifth is
// `bounds.defaultBeam`, read from the width curve, so a re-measured default moves
// the preset in the same commit as the Reference rows that depend on it (§9.5).
//
// A preset is URL state, not a separate mechanism: clicking one runs the same
// clamp → `replaceState` → re-solve path as typing in a field, which is why the
// goal 8 preset's link is the bare URL and the goal 12 preset's is
// `?goal=12&pieces=100`.

import type { Bounds } from "../core/game.ts";
import type { GameInput } from "../core/protocol.ts";

export interface Preset {
  /** §6's directory name: `goal8-p40-cap7`. */
  readonly id: string;
  /** The button's face. Short; the tooltip carries the rest. */
  readonly label: string;
  /** The four committed values. The fifth (`beam`) comes from `bounds`. */
  readonly settings: Readonly<Pick<GameInput, "seed" | "goal" | "pieces" | "heightCap">>;
  /** One line saying which published figure this button reproduces. */
  tooltip(bounds: Bounds): string;
}

/** A preset's game: its four committed values at the width the figures were measured at. */
export function presetGame(preset: Preset, bounds: Bounds): GameInput {
  return { ...preset.settings, beamWidth: bounds.defaultBeam };
}

export const PRESETS: readonly Preset[] = [
  {
    id: "goal8-p40-cap7",
    label: "Goal 8 · 40 pieces",
    settings: { seed: 0, goal: 8, pieces: 40, heightCap: 7 },
    tooltip: (bounds) =>
      `The committed configuration goal 8, 40 pieces, cap 7, seed 0, at the default width ` +
      `${bounds.defaultBeam} — the game the Reference run quotes, and the game the site opens on.`,
  },
  {
    id: "goal12-p100-cap7",
    label: "Goal 12 · 100 pieces",
    settings: { seed: 0, goal: 12, pieces: 100, heightCap: 7 },
    tooltip: (bounds) =>
      `The committed configuration goal 12, 100 pieces, cap 7, seed 0, at the default width ` +
      `${bounds.defaultBeam} — the harder game the Reference run quotes, paired with the first ` +
      `on seed 0: the same Board, a longer Sequence.`,
  },
];
