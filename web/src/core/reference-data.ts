// The committed Reference rows, as bytes in the bundle.
//
// `reference.jsonl` is an artifact ticket 09 appends to, so the page must not carry a
// copy of its numbers: the file is imported **raw** at build time (Vite's `?raw`
// query) and parsed by `core/reference.ts` at boot, which keeps "the reference column
// is the committed row" true by construction — there is no constant to go stale, and
// no fetch that can fail after the page has painted.
//
// The glob rather than two hard-coded paths: a third configuration landing in
// `data/dataset/` is picked up without editing this file, and a configuration whose
// file is missing simply has no row — the panel then says so in words instead of
// quoting a figure.
//
// This module is the only one that cannot be imported by `node --test` (the `?raw`
// query is a bundler feature); the parsing and the formatting it feeds live in
// `core/reference.ts`, which the tests do import, so the untested part is a file read.

import { latestReference, parseReferenceJsonl, type ReferenceRow } from "./reference.ts";

const FILES = import.meta.glob("../../../data/dataset/*/reference.jsonl", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

/** Every configuration's latest row, keyed by the Dataset directory's name. */
export const REFERENCE_ROWS: ReadonlyMap<string, ReferenceRow> = new Map(
  Object.entries(FILES)
    .sort(([left], [right]) => left.localeCompare(right))
    .flatMap(([path, text]) => {
      const name = path.split("/").at(-2) ?? "";
      const latest = latestReference(parseReferenceJsonl(text));
      return latest === null ? [] : ([[name, latest]] as [string, ReferenceRow][]);
    }),
);

/** The row to show beside a configuration's live column, or `null`. */
export function referenceFor(config: string): ReferenceRow | null {
  return REFERENCE_ROWS.get(config) ?? null;
}
