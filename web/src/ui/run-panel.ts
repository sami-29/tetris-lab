// The Run panel: two configurations, seven rows, two columns, two machine labels.
//
// The shape is notes/14 §2.2's R1–R9 and nothing new — this module fills the value
// slots the shell reserved. Three rules from the ticket are structural here:
//
//   * **a row is `figure | yours | reference`**, so "beside the Reference run" is a
//     row on screen and not a footnote;
//   * **the skeleton never changes** — every column head, figure label and label slot
//     exists from the first paint, and only `textContent` moves. A Run that has not
//     happened says `not measured in this tab yet`; a panel that went blank would
//     read as a failed load;
//   * **each column carries the machine that produced it**: `yours` says `this
//     browser`, the worker count and the clock; `reference` says the CPU, the OS, the
//     runtime, the engine commit and the date. A number without one of those is the
//     defect the ticket forbids.
//
// `data-run-cell="<config>-<figure>-<column>"` is the harness's handle: the headless
// check compares the live column with the reference column row by row, which is how
// "the same statistic, two machines" is asserted rather than asserted by eye.

import { NOT_MEASURED, createFigure, figureSpec, type FigureView } from "./figure.ts";
import { formatCells, type ConfigShape, type RunCells } from "../core/run-stats.ts";
import { referenceDetail, referenceLabel, referenceStats, type ReferenceRow } from "../core/reference.ts";

/** Stamp a paragraph as the R8 figure, with the registry's own words. */
function stampLabel(element: HTMLElement, tip: string): void {
  const spec = figureSpec("r8");
  element.dataset.figure = "r8";
  element.dataset.figureLabel = spec.label;
  element.dataset.figureSource = spec.source;
  element.dataset.figureKind = spec.kind;
  element.dataset.tip = tip;
}

/** The seven rows, in the notes' order, with the ids the registry already names. */
const ROWS = [
  { id: "r1", column: "winRate" },
  { id: "r2", column: "gamesWon" },
  { id: "r3", column: "failLinesMean" },
  { id: "r4", column: "failLinesMedian" },
  { id: "r5", column: "evals" },
  { id: "r6", column: "ms" },
  { id: "r7", column: "total" },
] as const;

export interface RunPanelOptions {
  readonly host: HTMLElement;
  readonly configs: readonly ConfigShape[];
  /** The committed row for a configuration, or `null` when none is committed. */
  readonly reference: (config: string) => ReferenceRow | null;
}

export interface RunPanel {
  /** One configuration's live cells, or `null` to go back to the placeholder. */
  paint(index: number, cells: RunCells | null): void;
  /** The words under the live column, and its tooltip. */
  paintYoursLabel(words: string, detail: string): void;
  /** The sentence under the table: a partial Run's note, a failure's reason, or null. */
  paintNote(words: string | null): void;
}

export function mountRunPanel(options: RunPanelOptions): RunPanel {
  const yours: FigureView[][] = [];
  const labelSlots: HTMLElement[] = [];

  for (const config of options.configs) {
    const block = document.createElement("section");
    // Not `run-config`: a CSS class name is a string literal in the bundle, and the
    // banned-words gate reads string literals as prose. `run-block` says the same thing
    // with a word the page may say.
    block.className = "run-block";
    block.dataset.config = config.name;

    const head = document.createElement("p");
    head.className = "run-cfg";
    head.textContent =
      `goal ${config.goal} · ${config.pieces} pieces · ` +
      `height ${config.cap} · beam ${config.beamWidth}`;
    head.dataset.tip =
      `One of the two committed configurations. Its test seeds are ${config.first}–${config.last} ` +
      `(${config.last - config.first + 1} games), solved at the shipped beam width.`;

    const table = document.createElement("table");
    table.className = "rtable";
    const thead = document.createElement("thead");
    const headRow = document.createElement("tr");
    for (const [label, tip] of [
      ["", "One row per statistic."],
      ["Yours", "Measured now, by this browser's workers. The label under the table names the machine."],
      ["Ref.", "The committed reference run: the same statistic on the machine named under the table."],
    ]) {
      const th = document.createElement("th");
      th.textContent = label;
      th.dataset.tip = tip;
      headRow.append(th);
    }
    thead.append(headRow);
    table.append(thead);

    const tbody = document.createElement("tbody");
    const cells: FigureView[] = [];
    const referenceRow = options.reference(config.name);
    const referenceCells = referenceRow ? formatCells(referenceStats(referenceRow)) : null;
    for (const row of ROWS) {
      const tr = document.createElement("tr");
      const name = document.createElement("td");
      name.textContent = figureSpec(row.id).label;
      name.dataset.tip = figureSpec(row.id).tip;

      const mine = document.createElement("td");
      const mineFigure = createFigure(row.id, NOT_MEASURED);
      mineFigure.set(NOT_MEASURED, "");
      mineFigure.el.classList.add("rtable-fig", "nolabel");
      mineFigure.el.dataset.runCell = `${config.id}-${row.id}-yours`;
      mine.append(mineFigure.el);
      cells.push(mineFigure);

      const theirs = document.createElement("td");
      theirs.className = "run-reference";
      const theirFigure = createFigure(row.id, referenceCells ? referenceCells[row.column] : "no Reference row");
      // Both columns carry their unit inside the cell, so neither keeps the registry's
      // unit element: "9,670/11,414" twice reads as a comparison, "…Evaluations" once
      // and not the other reads as a bug.
      theirFigure.set(referenceCells ? referenceCells[row.column] : "no Reference row", "");
      theirFigure.el.classList.add("rtable-fig", "nolabel");
      theirFigure.el.dataset.runCell = `${config.id}-${row.id}-reference`;
      if (referenceRow) theirFigure.el.dataset.tip = referenceDetail(referenceRow);
      theirs.append(theirFigure.el);

      tr.append(name, mine, theirs);
      tbody.append(tr);
    }
    table.append(tbody);
    const label = document.createElement("p");
    label.className = "refnote";
    label.dataset.runLabel = `${config.id}-yours`;
    stampLabel(label, NOT_MEASURED);
    label.textContent = NOT_MEASURED;
    labelSlots.push(label);

    const reference = document.createElement("p");
    reference.className = "refnote run-reference";
    reference.dataset.runLabel = `${config.id}-reference`;
    if (referenceRow) {
      stampLabel(reference, referenceDetail(referenceRow));
      reference.textContent = referenceLabel(referenceRow);
    } else {
      stampLabel(
        reference,
        "No Reference run row exists for this configuration, so the reference column quotes no figure at all. " +
          "`bench reference --append` writes one.",
      );
      reference.textContent = "no Reference row is committed yet";
    }

    block.append(head, table, label, reference);
    options.host.append(block);
    yours.push(cells);
  }

  // The partial/failed note. A separate line from the budget's assumption sentence,
  // because a stopped Run must not be able to overwrite what the budget is measured on.
  const note = document.createElement("p");
  note.className = "run-note";
  note.id = "run-partial";
  note.dataset.runLabel = "note";
  note.textContent = "";
  options.host.append(note);

  return {
    paint(index, cells) {
      const figures = yours[index];
      if (!figures) return;
      for (const [at, row] of ROWS.entries()) {
        // No unit element in these cells: each cell carries its unit in words
        // ("100.0 % of 1,000", "6.3/8.2 ms"), because three columns in a 236 px rail
        // have no room for a unit column, and a unit twice is worse than once.
        figures[at].set(cells ? cells[row.column] : NOT_MEASURED, "");
      }
    },
    paintYoursLabel(words, detail) {
      for (const label of labelSlots) {
        label.textContent = words;
        label.dataset.tip = detail;
      }
    },
    paintNote(words) {
      note.textContent = words ?? "";
    },
  };
}
