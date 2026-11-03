// The Run panel's words: the labels that say which machine produced which column.
//
// §2.4 wants the Reference run's rows shown "with its engine commit, wasm hash, build
// flags, CPU, OS, runtime, browser/Node and date labels", and the ticket wants each
// side labelled with the machine that produced it. A browser can only know part of
// that about itself, so this file writes down exactly the part it knows and refuses
// the rest:
//
//   * it **can** read `hardwareConcurrency`, the user agent, and the number of
//     workers it actually ran;
//   * it **cannot** read the CPU brand string — UA-CH exposes architecture and
//     bitness, not the model, and `navigator.userAgent` is frozen — so the `yours`
//     label never prints one (notes/15 §6.2, §9 Q5);
//   * it must say `this browser`, because the Reference row's substrate is a native
//     harness: the Evaluations are substrate-independent and the ms are not, so a
//     column that did not name its substrate would invite exactly the wrong
//     comparison.
//
// The mid-range 8-core laptop is the budget's sizing assumption and is stated as one,
// next to the numbers rather than in an about page: §9.3's own sentence.

/** The assumption the benchmark's 5 s budget is sized on. The machine differs; the check is the same. */
export const ASSUMPTION =
  "The 5 s budget was sized on a mid-range 8-core laptop. That is an assumption, not a measurement: " +
  "your column is whatever machine you ran it on.";

export interface YoursLabelInput {
  /** How many workers the Run actually used. */
  readonly workers: number;
  /** `navigator.hardwareConcurrency`, or 0 when the browser does not say. */
  readonly cores: number;
  readonly beam: number;
  /** A short browser name, e.g. `Chrome 141`; empty when it cannot be read. */
  readonly browser: string;
  /** A short OS name, e.g. `Windows`; empty when it cannot be read. */
  readonly os: string;
}

/**
 * The label under your column: the browser, the worker count against the core count,
 * and the fact that this was measured now, not committed.
 */
export function yoursLabel(input: YoursLabelInput): string {
  const cores = input.cores > 0 ? `${input.cores} logical cores` : "unknown core count";
  const parts = [
    "this browser",
    `${input.workers} ${input.workers === 1 ? "worker" : "workers"} of ${cores}`,
    input.browser,
    input.os,
    `beam ${input.beam}`,
    "measured now",
  ];
  return parts.filter((part) => part.length > 0).join(" · ");
}

/** The tooltip under your column: what a page can and cannot know about its own machine. */
export const YOURS_DETAIL =
  "Measured now by this browser's own workers. A page cannot read the CPU model, so the worker " +
  "count against the logical core count is the honest machine fact; times are quantised by this " +
  "browser's clock.";

/**
 * What a stopped run says: the count it stopped at, and a warning that the reference
 * column covers the whole split while your column covers only what was measured.
 */
export function partialNote(input: { readonly games: number; readonly total: number }): string {
  const games = input.games.toLocaleString("en-US");
  const total = input.total.toLocaleString("en-US");
  return (
    `${games} of ${total} games, stopped by hand — a partial run. The reference column is the ` +
    "whole split: 1,000 games per configuration."
  );
}

/** The status line while a Run is in flight, and what it becomes when it ends. */
export function progressWords(input: {
  readonly games: number;
  readonly total: number;
  readonly workMs: number;
  readonly workers: number;
  readonly status: "idle" | "running" | "complete" | "stopped" | "failed";
}): string {
  // The rail is 236 px and this line sits in the panel head beside `Run`: about 31
  // monospaced characters fit, so the sentence is the count and the work time and the
  // rest lives where it has room — the worker count in the `yours` label, the chunk
  // and Evaluation counters in R9's tooltip, both configurations' totals in R7.
  const games = input.games.toLocaleString("en-US");
  const total = input.total.toLocaleString("en-US");
  const work = `${(input.workMs / 1000).toFixed(1)} s`;
  switch (input.status) {
    case "running":
      return `${games}/${total} games · ${work}`;
    case "complete":
      return `${games}/${total} games · ${work}`;
    case "stopped":
      return `partial · ${games}/${total} games`;
    case "failed":
      return `failed · ${games}/${total} games`;
    default:
      return "not run yet";
  }
}
