// The two numbers the page hangs on, read at build time from the committed width
// curve (§9.5): `DEFAULT_BEAM` is the width the toolbar opens at, `WIDTH_CAP` is
// the slider's maximum, the URL's clamp and — through `DEFAULT_BEAM` — the traced
// band's width.
//
// This is the *only* module in the page allowed to name either number, and it
// names them by importing the artifact `bench width-curve` writes. There is no
// copy, no constant to keep in step and nothing to fetch at runtime: Vite's JSON
// plugin turns the artifact into an ES module at build time, and the named
// re-exports let Rollup tree-shake the rest of the curve out of the bundle.
//
// `web/test/width-curve.test.mjs` imports this module and compares what it hands
// back against a fresh parse of the artifact, so a hand-edited artifact, a stale
// copy or a hard-coded width in `main.ts` fails a test rather than shipping.
//
// The import attribute is deliberate: Vite accepts it, and it lets `node --test`
// import this file directly (Node refuses a JSON import without one), which is
// what makes the drift guard a real import instead of a text comparison.
//
// Node exposes a JSON module through `default` only; it does not synthesise
// named exports from the object's keys the way Vite does. So the artifact is
// imported whole and the two numbers are projected here. That costs ~2 KB of
// gzipped bundle (the whole curve, instead of two integers) and buys a module
// that Node and Vite agree on — the alternative was a check that only ran under
// one of them, which is exactly the drift this file exists to prevent.

import curve from "../../../data/measurements/width-curve.json" with { type: "json" };

export const DEFAULT_BEAM: number = curve.default_beam;
export const WIDTH_CAP: number = curve.width_cap;
