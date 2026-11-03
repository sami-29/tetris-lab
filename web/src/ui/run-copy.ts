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

/** The assumption the 5 s budget is sized on, in §9.3's words and not a claim more. */
export const ASSUMPTION =
  "What the 5 s budget is measured on: the reference rows were measured on the reference box by a " +
  "native harness. A mid-range 8-core laptop is an assumption used to size the budget — it is not " +
  "a measurement, and no figure on this page was produced on one. Your column is this machine and " +
  "this browser.";

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
 * The label under the live column. Provenance, not decoration: the browser, the OS,
 * the worker count against the core count, the beam the Run used, and the fact that
 * it was measured just now rather than committed.
 */
export function yoursLabel(input: YoursLabelInput): string {
  const cores = input.cores > 0 ? `${input.cores} logical cores` : "the cores this browser reports";
  const parts = [
    "this browser",
    `${input.workers} ${input.workers === 1 ? "worker" : "workers"} of ${cores}`,
    input.browser,
    input.os,
    `beam ${input.beam}`,
    "measured just now",
  ];
  return parts.filter((part) => part.length > 0).join(" · ");
}

/** The tooltip under the live column: what the page cannot know, said out loud. */
export const YOURS_DETAIL =
  "This column was measured in this tab, just now, by this browser's own workers: each game is a " +
  "`solve_batch` call timed with performance.now(), because the engine cannot time itself (its " +
  "clock traps in wasm). A browser does not expose the CPU model, so the worker count against the " +
  "logical core count is the honest machine fact here; the ms rows are quantised by this " +
  "browser's clock, which is 0.1 ms in a page that is not cross-origin isolated.";

/**
 * What a stopped Run says. Two jobs: name the count it stopped at, and refuse the
 * comparison a partial Run cannot support — a prefix of the split is not the split,
 * and its ms rows would quote a mean over the seeds it happened to reach.
 */
export function partialNote(input: { readonly games: number; readonly total: number }): string {
  const games = input.games.toLocaleString("en-US");
  const total = input.total.toLocaleString("en-US");
  return (
    `partial Run — stopped by hand after ${games} of ${total} games. The reference column is the ` +
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
      // The word `partial` is the point of the sentence: what is on screen is not the
      // Split, and the ms rows are a mean over a prefix of the seeds.
      return `partial · ${games}/${total} games`;
    case "failed":
      return `failed · ${games}/${total} games`;
    default:
      return "no Run in this tab yet";
  }
}
