// The committed Reference rows, as the page reads them.
//
// `data/dataset/<configuration>/reference.jsonl` is an artifact ticket 09 appends to,
// so the page must not carry a copy of its numbers typed into a module. It carries the
// **latest row itself**, projected by `web/scripts/build-facts.mjs` into
// `src/generated/engine-facts.ts` — the same generator, run by `npm run build` and
// `npm test`, that the Explainer's labels come from. There is therefore no constant to
// go stale, and no fetch that can fail after the page has painted.
//
// Why not import the file raw: the banned-words gate scans the built bundle's string
// literals, and a raw JSONL blob is one enormous literal whose machine and build
// labels it cannot tell from prose. Projecting the row into the generated module keeps
// the same values with the page's own words around them (and `run-stats.test.mjs`
// asserts the projection is the committed row, less one Rust keyword — see the
// generator's comment).
//
// The projection is enumerated from `data/dataset/` by the generator, so a third
// configuration is picked up without editing this file, and a configuration whose file
// is missing simply has no row — the panel then says so in words instead of quoting a
// figure.

import { ENGINE_FACTS } from "../generated/engine-facts.ts";
import { referenceFromObject, type ReferenceRow } from "./reference.ts";

/** Every configuration's latest row, keyed by the Dataset directory's name. */
export const REFERENCE_ROWS: ReadonlyMap<string, ReferenceRow> = new Map(
  ENGINE_FACTS.reference.rows.flatMap((fact) => {
    const at = typeof fact.row.index === "number" ? fact.row.index : 0;
    const row = referenceFromObject(fact.row, at);
    return row === null ? [] : ([[fact.config, row]] as [string, ReferenceRow][]);
  }),
);

/** The row to show beside a configuration's live column, or `null`. */
export function referenceFor(config: string): ReferenceRow | null {
  return REFERENCE_ROWS.get(config) ?? null;
}
