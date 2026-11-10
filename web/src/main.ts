// The page: a workbench, not a form.
//
// Five regions (§2) — toolbar, left rail, centre, right inspector, status bar — plus
// the Explainer drawer, in one page that does not scroll and grows into the viewport
// height. The grid is `styles/shell.css`; this file is the wiring and the words.
//
// What each wave of tickets left here, and what must survive:
//
//   * **10** owns playback: `ui/playback.ts` draws every frame from `(k, into)`,
//     `ui/clock.ts` the playhead, `ui/timeline.ts` the boards. The transport mounts
//     around them; nothing in this file animates.
//   * **11** owns the trace, the band and the lineage (`ui/band-model.ts`,
//     `ui/band-canvas.ts`). The inspector mounts them.
//   * **13** owns the Playground and the URL: one commit path, `replaceState`, one
//     solve in flight with one queued. Unchanged.
//   * **14** — this ticket — owns the regions, the figure table, the tooltips, the
//     icons, the drawer and the status bar. It replaces the *shell*, not the
//     behaviour inside it.
//
// The single rule that shapes the code below: **a number on screen is a
// `[data-figure]` with a words label and a provenance**, and it is either the
// engine's, the trace's, a replay's, the URL's, a committed artifact's, or this
// page's own clock. Where a ticket has not landed yet (the Run panel, the tree) the
// value slot says so in words — never a dash, never a guess.
//
// `ui/figure.ts` is the table of those figures; this file fills their slots.

import "./styles/tokens.css";
import "./styles/shell.css";

import engineWasmUrl from "../pkg/engine.wasm?url";

import { DEFAULT_BEAM, WIDTH_CAP } from "./config/width.ts";
import { SHIPPED_EVALUATOR, decimalOf, weightUnit } from "./config/evaluator.ts";
import {
  clampGame,
  defaultGame,
  describeGame,
  parseGame,
  reSolveReason,
  serialiseGame,
  type Bounds,
} from "./core/game.ts";
import type { FromWorker, GameMessage, GameInput, ToWorker } from "./core/protocol.ts";
import { H, W, emptyField, fieldFrom, type Field } from "./ui/field.ts";
import { BoardView } from "./ui/board.ts";
import { BandView } from "./ui/band-canvas.ts";
import { buildBand, countLines, decodeTrace, winningLineage, type BandModel } from "./ui/band-model.ts";
import type { Trace } from "./ui/trace.ts";
import { Playback, type PlaybackStatus } from "./ui/playback.ts";
import { Playground } from "./ui/playground.ts";
import { letterOf } from "./ui/pieces.ts";
import { MPS_DEFAULT, MPS_MAX, MPS_MIN, mpsToSlider, msPerMove, sliderToMps } from "./ui/phases.ts";
import { KEY_MAP, SPEED_STOP, resolveKey, type Action } from "./ui/keys.ts";
import { moveToX, stripMode, xToMove } from "./ui/seek.ts";
import { createFigure, type FigureId, type FigureView } from "./ui/figure.ts";
import { mountIcon, pieceGlyph, svgIcon, svgElement, type IconName } from "./ui/icons.ts";
import { adoptTitles, installTooltips } from "./ui/tooltip.ts";
import { mountExplainer } from "./ui/explainer.ts";
// The Run. `core/pool.ts` owns the workers, the chunk queue and the totals
// (and is DOM-free, so its properties are asserted in `node --test`);
// `ui/run-panel.ts` fills the benchmark's value slots; `ui/run-copy.ts` holds the
// words that say which machine produced which column.
import { createRunPool, poolSize, type RunPool } from "./core/pool.ts";
import { formatCells, statsOf, type ConfigShape } from "./core/run-stats.ts";
import { referenceFor } from "./core/reference-data.ts";
import { mountRunPanel } from "./ui/run-panel.ts";
import { YOURS_DETAIL, partialNote, progressWords, yoursLabel } from "./ui/run-copy.ts";
import { ENGINE_FACTS } from "./generated/engine-facts.ts";
import { columnProfile, profileScale, profileTip } from "./ui/readout.ts";
import { layerRows } from "./ui/inspector.ts";
// Ticket 12's drill-down, mounted rather than re-implemented: the same decoded trace
// the band read, drawn as the top states of each Layer. It owns its own view switch
// and its own bound, and it never reaches the solver (its module comment says so).
import { TreePanel } from "./ui/tree-panel.ts";
import { Tour, tourSeen } from "./ui/tour.ts";
import { reconstruct } from "./ui/timeline.ts";
import type { TreeRef } from "./ui/tree-model.ts";

/** §4's handshake number, echoed in the status bar's boot slot. */
const ABI = 2;

/**
 * The two numbers the page hangs on, read at build time from the committed width
 * curve through `./config/width.ts` — the one module allowed to name them.
 */
const BOUNDS: Bounds = { defaultBeam: DEFAULT_BEAM, widthCap: WIDTH_CAP };

/** Every element the page needs, or a loud failure at boot. */
function must<T extends Element>(selector: string): T {
  const element = document.querySelector<T>(selector);
  if (!element) throw new Error(`index.html is missing ${selector}`);
  return element;
}

/** A plain element with a class, in one line. */
function el<K extends keyof HTMLElementTagNameMap>(tag: K, className = ""): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);
  if (className) element.className = className;
  return element;
}

// ── the elements ─────────────────────────────────────────────────────────────
const app = must<HTMLElement>("#app");
const canvas = must<HTMLCanvasElement>("#board");
const statusLine = must<HTMLElement>("#status");
const outcomeLine = must<HTMLElement>("#outcome");
const bootSlot = must<HTMLElement>("#boot");
const playButton = must<HTMLButtonElement>("#play");
const playIcon = must<HTMLElement>("#play-icon");
const playWord = must<HTMLElement>("#play-word");
const prevButton = must<HTMLButtonElement>("#prev");
const nextButton = must<HTMLButtonElement>("#next");
const restartButton = must<HTMLButtonElement>("#restart");
const solveButton = must<HTMLButtonElement>("#solve");
const howButton = must<HTMLButtonElement>("#how");
const expertButton = must<HTMLButtonElement>("#expert");
const speedSlider = must<HTMLInputElement>("#speed");
const speedReadout = must<HTMLOutputElement>("#speed-readout");
const speedFigureNum = speedReadout.querySelector<HTMLElement>(".num")!;
const speedFigureUnit = speedReadout.querySelector<HTMLElement>(".unit")!;
const strip = must<HTMLElement>("#strip");
const caret = must<HTMLElement>("#caret");
const sheet = must<HTMLElement>("#sheet");
const sheetList = must<HTMLElement>("#sheet-list");
const drawer = must<HTMLElement>("#drawer");
const scrim = must<HTMLElement>("#scrim");
const drawerClose = must<HTMLButtonElement>("#drawer-close");
const explainerBody = must<HTMLElement>("#explainer-body");
const playgroundRoot = must<HTMLElement>("#playground");
const runFirstInput = must<HTMLInputElement>("#run-first");
const runCountInput = must<HTMLInputElement>("#run-count");
const runStartButton = must<HTMLButtonElement>("#run-start");
const runCustomStopButton = must<HTMLButtonElement>("#run-yours-stop");
const runCustomNote = must<HTMLElement>("#run-yours-note");
const bandCanvas = must<HTMLCanvasElement>("#band");
const bandPanel = must<HTMLElement>("#band-panel");
const bandLabel = must<HTMLElement>("#band-label");
const bandCaption = must<HTMLElement>("#band-caption");
const bandCounts = must<HTMLElement>("#band-counts");
const bandLegend = must<HTMLElement>("#band-legend");
const inspectorViews = must<HTMLElement>("#views");
const treePanelRoot = must<HTMLElement>("#tree-panel");
const treeNote = must<HTMLElement>("#tree-note");
const treeState = must<HTMLElement>("#tree-state");
const treeBoundSlider = must<HTMLInputElement>("#tree-bound");
const layerCountSlot = must<HTMLElement>("#layer-count");
const layerRowsBody = must<HTMLElement>("#layer-rows");
const layerLine = must<HTMLElement>("#layer-line");
const pieceNow = must<HTMLElement>("#piece-now");
const gutter = must<HTMLElement>("#gutter");
const roScore = must<HTMLElement>("#ro-score");
const roColumns = must<HTMLElement>("#ro-columns");
const roPlacement = must<HTMLElement>("#ro-placement");
const roNext = must<HTMLElement>("#ro-next");
const statusFigures = must<HTMLElement>("#status-figures");
const engineFigures = must<HTMLElement>("#engine-figures");
const engineWeights = must<HTMLElement>("#engine-weights");
const runBody = must<HTMLElement>("#run-body");
const runState = must<HTMLElement>("#run-state");
const runButton = must<HTMLButtonElement>("#run-dataset");
const stopButton = must<HTMLButtonElement>("#run-stop");
const moveFigureHost = must<HTMLElement>("#move-figure");
const panelRun = must<HTMLElement>("#panel-run");
const layerPanel = must<HTMLElement>("#layer-panel");
const modeButtons = [...must<HTMLElement>("#modes").querySelectorAll<HTMLButtonElement>("[data-mode]")];
const tourOpenButton = must<HTMLButtonElement>("#tour-open");
const tourStartButton = must<HTMLButtonElement>("#tour-start");

const view = new BoardView(canvas);
const bandView = new BandView(bandCanvas);
/**
 * The inspector's three views: the band (default), the tree (drill-down) and the
 * per-layer table. Ticket 12's panel owns the switch — `options.views` is the tab strip
 * above all three — and the selected state's card, whose Board this page answers from
 * the replay.
 */
const treePanel = new TreePanel(treePanelRoot, {
  bandPanel,
  layersPanel: layerPanel,
  views: inspectorViews,
  onSelection: () => syncTreeNote(),
});

// ── the shell: icons, figures, tooltips ──────────────────────────────────────

/** Every `[data-icon]` placeholder gets its hand-made SVG. */
for (const host of document.querySelectorAll<HTMLElement>("[data-icon]")) {
  mountIcon(host, host.dataset.icon as IconName);
}
mountIcon(document.getElementById("play-icon")!, "play", 10);

/** The figures, by id, so an update is one lookup and one `set`. */
const FI: Partial<Record<FigureId, FigureView>> = {};

function fig(id: FigureId, host: HTMLElement, initial?: string): FigureView {
  const built = createFigure(id, initial);
  host.append(built.el);
  FI[id] = built;
  return built;
}

// The Engine panel: the width (the same setting the Playground's slider carries) and,
// behind `Expert`, the shipped Evaluator's eight Weights with their bounds.
const e1 = fig("e1", engineFigures, String(DEFAULT_BEAM));
for (const [index, feature] of SHIPPED_EVALUATOR.features.entries()) {
  const weight = fig(`w${index + 1}` as FigureId, engineWeights);
  weight.set(decimalOf(feature.weightQ16), `${weightUnit(feature)} · 0…${feature.bound[1]}`);
  weight.el.dataset.tip = `${feature.label} — the paper's words: "${feature.paper}". Weight ${decimalOf(feature.weightQ16)} of Q16 ${feature.weightQ16.toLocaleString("en-US")}, charged against a Feature bounded 0…${feature.bound[1]}. The integer is the contract; the decimal is its rendering.`;
}
const evaluatorNote = el("p", "note");
evaluatorNote.textContent = `${SHIPPED_EVALUATOR.name} — ${SHIPPED_EVALUATOR.source}. Read-only: no ABI export returns a weight, so the page shows the table the engine was built with, and a test compares every integer against the Rust.`;
engineWeights.append(evaluatorNote);

// ── the Run ────────────────────────────────────────────────────────────────
//
// The two committed configurations, from the manifests ticket 02 generated into
// `engine-facts.ts` — never typed here. Both run at the shipped beam width: a
// comparison across widths is not a comparison, and the Reference rows are at 25.
const RUN_CONFIGS: readonly ConfigShape[] = ENGINE_FACTS.dataset.configs
  .map((config) => ({
    name: config.name,
    goal: config.goal,
    pieces: config.pieces,
    cap: config.cap,
    beamWidth: DEFAULT_BEAM,
    first: config.test.first,
    last: config.test.last,
  }))
  .sort((left, right) => left.goal - right.goal)
  .map((config, index) => ({ ...config, id: index as 0 | 1 }));

const RUN_GAMES = RUN_CONFIGS.reduce((sum, config) => sum + (config.last - config.first + 1), 0);

// The assumption is a fixed line under the benchmark table, not a tooltip: it
// qualifies the budget the ms and total rows are read against. It is appended to a
// slot the markup carries, so first paint already holds its words.
const runPanel = mountRunPanel({ host: runBody, configs: RUN_CONFIGS, reference: referenceFor });
// The static words in the markup are the first paint's; from here the figure owns the
// slot, so the skeleton sentence must not survive underneath it.
runState.replaceChildren();
const r9 = fig("r9", runState);
r9.el.classList.add("nolabel");
r9.set("not run yet", "");
runPanel.paintNote(null);
// The assumption sentence is static in the markup; leave the first paint's words.

let runPool: RunPool | null = null;
let runStartedAt = 0;

/** This thread's clock for the Run, which includes the message hops. */
const runElapsed = (): number => (runStartedAt === 0 ? 0 : performance.now() - runStartedAt);

function paintRunProgress(
  status: "idle" | "running" | "complete" | "stopped" | "failed",
  workers: number,
): void {
  const progress = runPool?.progress ?? { chunkDone: 0, gamesDone: 0, evals: 0, ms: 0 };
  runState.dataset.runStatus = status;
  runState.dataset.workers = String(workers);
  r9.set(
    progressWords({
      games: progress.gamesDone,
      total: RUN_GAMES,
      workMs: progress.ms,
      workers,
      status,
    }),
    "",
  );
  // The four counters §2.4 names, each labelled, in the tooltip as well as the status
  // line: `gamesDone` and work time are the words that fit a 236 px rail, and
  // `chunkDone` and the Evaluations behind them are what make the work rate readable
  // (Evaluations per millisecond is a throughput, so a slow machine is distinguishable
  // from a stuck one rather than guessed at).
  const cores = navigator.hardwareConcurrency ?? 1;
  r9.el.dataset.tip =
    `Run progress — ${progress.gamesDone.toLocaleString("en-US")} of ` +
    `${RUN_GAMES.toLocaleString("en-US")} games measured or status-coded, across ` +
    `${progress.chunkDone.toLocaleString("en-US")} chunks, with ` +
    `${progress.evals.toLocaleString("en-US")} Evaluations behind them and ` +
    `${(progress.ms / 1000).toFixed(1)} s of work in ${workers} ` +
    `${workers === 1 ? "worker" : "workers"} of this machine's ${cores} logical cores. ` +
    "Work time is the sum of the chunks' own clocks — the quantity a Reference row's " +
    "batch_ms is — not this tab's elapsed wall clock.";
}

/** Repaint both configurations' live cells from the pool's own totals. */
function paintRunCells(partial: boolean): void {
  const pool = runPool;
  if (!pool) return;
  for (const [index, totals] of pool.totals().entries()) {
    runPanel.paint(
      index,
      formatCells(statsOf(totals), {
        live: true,
        partial,
        clock: 0.1,
        elapsedMs: runElapsed(),
      }),
    );
  }
}

/** A short, honest browser and OS name: a page cannot read the CPU model. */
function browserName(): string {
  const agent = navigator.userAgent;
  const known: readonly (readonly [RegExp, string])[] = [
    [/Edg\/(\d+)/, "Edge"],
    [/OPR\/(\d+)/, "Opera"],
    [/Firefox\/(\d+)/, "Firefox"],
    [/Chrome\/(\d+)/, "Chrome"],
    [/Version\/(\d+)[\d.]* Safari/, "Safari"],
  ];
  for (const [pattern, name] of known) {
    const found = pattern.exec(agent);
    if (found) return `${name} ${found[1]}`;
  }
  return "";
}

function osName(): string {
  const agent = navigator.userAgent;
  if (/Windows NT 10/.test(agent)) return "Windows 10/11";
  if (/Windows/.test(agent)) return "Windows";
  if (/Mac OS X/.test(agent)) return "macOS";
  if (/Android/.test(agent)) return "Android";
  if (/Linux/.test(agent)) return "Linux";
  return "";
}

/** One worker, booting the artifact itself: the wasm is fetched in the worker. */
function spawnRunWorker(): Worker {
  const created = new Worker(new URL("./workers/solve.worker.ts", import.meta.url), {
    type: "module",
  });
  const boot: ToWorker = { type: "boot", wasmUrl: engineWasmUrl };
  created.postMessage(boot);
  return created;
}

/**
 * §2.4's one action: solve the test Split of both configurations in this tab.
 *
 * Nothing is measured until this is pressed, and every figure the panel then shows
 * comes from a chunk the pool applied — never from an estimate. The worker count is
 * §7's `clamp(hardwareConcurrency - 1, 1, 8)`, and it is *this* machine's fact, which
 * is why the label under the column names it.
 */
function startRun(): void {
  if (anyRunBusy()) return;
  const cores = navigator.hardwareConcurrency ?? 1;
  const workers = poolSize(cores);
  runStartedAt = performance.now();
  runPanel.paintYoursLabel(
    yoursLabel({ workers, cores, beam: DEFAULT_BEAM, browser: browserName(), os: osName() }),
    YOURS_DETAIL,
  );
  runPanel.paintNote(null);
  for (const [index] of RUN_CONFIGS.entries()) runPanel.paint(index, null);
  runButton.disabled = true;
  runStartButton.disabled = true;
  stopButton.disabled = false;
  const pool = createRunPool({
    spawn: spawnRunWorker,
    workers,
    configs: RUN_CONFIGS,
    onProgress: () => {
      paintRunProgress("running", workers);
      paintRunCells(false);
    },
    onChunk: () => paintRunCells(false),
    onLog: (line, tone) => log(line, tone),
  });
  runPool = pool;
  log(`Run started: ${RUN_GAMES} games · ${workers} workers of ${cores} logical cores · beam ${DEFAULT_BEAM}`);
  paintRunProgress("running", workers);

  void pool.start().then((outcome) => {
    if (outcome.status === "complete") {
      paintRunCells(false);
      paintRunProgress("complete", workers);
      log(
        `Run complete: ${outcome.progress.gamesDone} games · ${(outcome.elapsedMs / 1000).toFixed(2)} s wall clock on ${workers} workers · ${outcome.chunks} chunks`,
      );
    } else if (outcome.status === "stopped") {
      // Partial, and labelled as partial: the denominators are in the cells and the
      // note says the reference column is the whole split.
      paintRunCells(true);
      paintRunProgress("stopped", workers);
      runPanel.paintNote(partialNote({ games: outcome.progress.gamesDone, total: RUN_GAMES }));
    } else {
      // A Run that could not measure publishes no rows at all: the slots go back to
      // their placeholder and the reason is the sentence on screen.
      for (const [index] of RUN_CONFIGS.entries()) runPanel.paint(index, null);
      paintRunProgress("failed", workers);
      runPanel.paintNote(`the Run stopped without a result — ${outcome.reason}`);
      log(`Run failed: ${outcome.reason}`, "error");
    }
    runButton.disabled = false;
    runStartButton.disabled = false;
    stopButton.disabled = true;
  });
}

runButton.addEventListener("click", () => startRun());
// Immediate, because it is `terminate()` on every worker rather than a flag a
// synchronous wasm loop could ignore (§7).
stopButton.addEventListener("click", () => runPool?.stop());

// ── your own run: the Playground's settings, over seeds you pick ──────────

let customPool: RunPool | null = null;
let customStartedAt = 0;
const customElapsed = (): number => (customStartedAt === 0 ? 0 : performance.now() - customStartedAt);

/** The seed range the form names, clamped to the engine's domain. */
function readSeedRange(): { first: number; count: number } {
  const maxSeed = 4294967295;
  const first = Math.max(0, Math.min(maxSeed, Math.floor(Number(runFirstInput.value) || 0)));
  let count = Math.max(1, Math.min(4000, Math.floor(Number(runCountInput.value) || 1)));
  if (first + count - 1 > maxSeed) count = maxSeed - first + 1;
  return { first, count };
}

/** The one benchmark and the one custom run never run together. */
function anyRunBusy(): boolean {
  return runPool?.status === "running" || customPool?.status === "running";
}

function setStartButtons(busy: boolean): void {
  runButton.disabled = busy;
  runStartButton.disabled = busy;
  stopButton.disabled = true;
  runCustomStopButton.disabled = true;
}

/** The sentence your run's note carries: progress while running, results when done. */
function paintCustomProgress(
  status: "idle" | "running" | "complete" | "stopped" | "failed",
  workers: number,
): void {
  const progress = customPool?.progress ?? { gamesDone: 0, chunkDone: 0, evals: 0, ms: 0 };
  const started = readSeedRange();
  runState.dataset.runStatus = status;
  runState.dataset.workers = String(workers);
  runState.textContent = progressWords({
    games: progress.gamesDone,
    total: started.count,
    workMs: progress.ms,
    workers,
    status,
  });
  const totals = customPool?.totals()[0];
  const measured = totals !== undefined && totals.n !== 0 && status !== "running";
  if (!measured) {
    const cells = totals === undefined ? null : formatCells(statsOf(totals), {
      live: true,
      partial: false,
      clock: 0.1,
      elapsedMs: customElapsed(),
    });
    runCustomNote.textContent = cells?.winRate
      ? `Win rate ${cells.winRate} · ${progress.gamesDone.toLocaleString("en-US")} of ` +
        `${started.count.toLocaleString("en-US")} games…`
      : `${progress.gamesDone.toLocaleString("en-US")} of ${started.count.toLocaleString("en-US")} games · ` +
        `${(progress.ms / 1000).toFixed(1)} s`;
    return;
  }
  const cells = formatCells(statsOf(totals), {
    live: true,
    partial: status === "stopped",
    clock: 0.1,
    elapsedMs: customElapsed(),
  });
  if (!cells) {
    runCustomNote.textContent = "No game answered.";
    return;
  }
  const parts: string[] = [];
  parts.push(`Win rate ${cells.winRate} · ${cells.ms} ms per game`);
  if (cells.evals) parts.push(cells.evals);
  if (cells.failLinesMean) parts.push(`${cells.failLinesMean} lines on losses`);
  if (status === "stopped") parts.unshift(`Partial, ${progress.gamesDone.toLocaleString("en-US")} games —`);
  runCustomNote.textContent = parts.join(" · ");
}

function startCustomRun(): void {
  if (anyRunBusy()) return;
  const { first, count } = readSeedRange();
  runFirstInput.value = String(first);
  runCountInput.value = String(count);
  const workers = poolSize(navigator.hardwareConcurrency ?? 1);
  const config: ConfigShape = {
    id: 0,
    name: "your seeds",
    goal: game.goal,
    pieces: game.pieces,
    cap: game.heightCap,
    beamWidth: game.beamWidth,
    first,
    last: first + count - 1,
  };
  setStartButtons(true);
  runCustomStopButton.disabled = false;
  customStartedAt = performance.now();
  const pool = createRunPool({
    spawn: spawnRunWorker,
    workers,
    configs: [config],
    onProgress: () => paintCustomProgress("running", workers),
    onChunk: () => paintCustomProgress("running", workers),
    onLog: (line, tone) => log(line, tone),
  });
  customPool = pool;
  runState.textContent = `0 of ${count} games · ${workers} ${workers === 1 ? "worker" : "workers"}`;
  runCustomNote.dataset.tip =
    `Seed ${first} up, goal ${game.goal} lines, ${game.pieces} pieces, starting height ${game.heightCap}, ` +
    `beam ${game.beamWidth}. Timings are this browser's; change the Playground to change the settings.`;
  void pool.start().then((outcome) => {
    if (outcome.status === "complete") {
      paintCustomProgress("complete", workers);
    } else if (outcome.status === "stopped") {
      paintCustomProgress("stopped", workers);
    } else {
      runState.textContent = "failed";
      runCustomNote.textContent = outcome.reason;
    }
    runState.dataset.runStatus = outcome.status;
    runState.dataset.workers = String(workers);
    customPool = null;
    setStartButtons(false);
  });
}

runStartButton.addEventListener("click", () => startCustomRun());
runCustomStopButton.addEventListener("click", () => customPool?.stop());
runCustomNote.textContent = "Nothing measured yet.";

// The status bar's Evaluations count: the one solve number shown nowhere else.
const s1 = fig("s1", statusFigures, "0");

// The move figure in the transport. The strip's aria-valuemax already carries the
// total for assistive technology, so this is the only visible one.
const m2 = fig("m2", moveFigureHost, "0");
m2.el.classList.add("nolabel");

// The board header's line: the Layer on screen.
layerLine.replaceChildren();
const c1 = fig("c1", layerLine, "Layer 0");

// The readout, section by section.
const c5 = fig("c5", roScore, "no solve yet");
const c6 = createFigure("c6");
c6.el.classList.add("fig-picture");
const cols = el("div", "cols");
const caps = el("div", "caps");
c6.el.classList.add("nolabel");
c6.el.append(cols, caps);
roColumns.append(c6.el);
FI.c6 = c6;
const c7 = fig("c7", roPlacement, "Move 0 is the starting board");
const c8 = fig("c8", roPlacement, "not placed");
const c9 = fig("c9", roPlacement, "not placed");
const c10 = fig("c10", roPlacement, "0");
const c11 = fig("c11", roPlacement, "0");
const c12 = createFigure("c12");
c12.el.classList.add("fig-picture");
const nextSeq = el("span", "seq");
c12.el.classList.add("nolabel");
c12.el.append(nextSeq);
roNext.append(c12.el);
FI.c12 = c12;

// The inspector's legend: the four things the band draws (§2.5 I1–I4), the best score
// in the Layer on screen (I5), and the selection card (I11).
fig("i1", bandLegend, "rows by score");
const i2 = fig("i2", bandLegend, "not measured");
i2.el.classList.add("nolabel");
const i3 = fig("i3", bandLegend, "winning lineage");
i3.el.classList.add("ln", "nolabel");
const i4 = fig("i4", bandLegend, "worst third · middle · best 18%");
i4.el.classList.add("nolabel");
for (const className of ["t3", "t2", "t1"]) {
  const dot = el("i", className);
  i4.el.prepend(dot);
}
const i5 = fig("i5", bandLegend, "not measured");
i5.el.classList.add("nolabel");
const i11 = fig("i11", treeState, "no state selected");
i11.el.classList.add("nolabel");

// The tree's head: ticket 12's panel draws into `#tree`, `#tree-caption` and
// `#tree-bound-readout`; these three figures are the same numbers in the ticket's own
// words, and `syncTreeNote()` keeps them the panel's rather than a copy of it.
treeNote.replaceChildren();
const i6 = fig("i6", treeNote, String(treePanel.treeBound));
const i7 = fig("i7", treeNote, "not drawn yet");
const i8 = fig("i8", treeNote, "the tree is closed");
for (const view of [i6, i7, i8]) view.el.classList.add("nolabel");

// The per-layer table's head count.
layerCountSlot.replaceChildren();
const i9 = fig("i9", layerCountSlot, "no layers yet");
i9.el.classList.add("nolabel");

// ── the log: console only. Diagnostics belong to devtools, not the screen. ─
function log(words: string, tone: "plain" | "error" = "plain"): void {
  if (tone === "error") console.error(words);
  else console.info(words);
}

// ── the board's row ruler (C3): twenty static numbers, 0 at the top ─────────
for (let row = 0; row < H; row += 1) {
  const number = el("i");
  number.textContent = String(row);
  gutter.append(number);
}

// ── state ───────────────────────────────────────────────────────────────────
let worker: Worker | null = null;
let playback: Playback | null = null;
/** The game last shown, for the readouts that outlive a frame. */
let shown: GameMessage | null = null;
/** The decoded trace, so the table, the band and the readout share one decode. */
let traced: Trace | null = null;
/**
 * The winning lineage's rank per Layer, for the drill-down's card and its amber path:
 * the one thing the tree's state card needs from the band.
 */
let traceLineage: TreeRef[] = [];
/** The band's last model, so a resize redraws it instead of blanking it. */
let bandModel: BandModel | null = null;
let startTimer = 0;
let nextRequestId = 1;
let pending = 0;
/** What the strip must repaint, so a 200-Move strip is not rewritten 60 times a second. */
let painted = { k: -1, total: -1 };
/** The speed survives a new Solve: it is the visitor's setting, not the game's. */
let speed = MPS_DEFAULT;
/** Set while a pointer drag on the strip is in progress, so we resume afterwards. */
let dragging = false;
let resumeAfterDrag = false;
/** Which Moves clear rows, computed once per game from the replay. */
let clearedMoveFlag: boolean[] = [];

/**
 * The game the page is showing, and the game it has asked for. They differ only
 * while a solve is in flight, and the URL always names the one that was *asked for*.
 */
let game: GameInput = defaultGame(BOUNDS);
/** The Board on screen, so a resize repaints what is there rather than blanking it. */
let shownBoard: Field = emptyField();
let booted = false;
let inFlight = false;
let queued: GameInput | null = null;
/** This Solve's wall clock, as the worker measured it: the page's `elapsed`. */
let elapsedMs = 0;
/** `solving` until the first answer; then `solved` or `lost`. Never an error tone. */
let outcome: "solving" | "solved" | "lost" = "solving";

function say(element: HTMLElement, text: string, tone: "plain" | "error" = "plain"): void {
  element.textContent = text;
  element.dataset.tone = tone;
}

// ── the worker: boot, one solve in flight, one queued ───────────────────────
function spawn(): void {
  if (worker) worker.terminate();
  booted = false;
  inFlight = false;
  worker = new Worker(new URL("./workers/solve.worker.ts", import.meta.url), {
    type: "module",
  });
  worker.onmessage = (event: MessageEvent<FromWorker>) => receive(event.data);
  worker.onerror = (event: ErrorEvent) => {
    say(statusLine, `worker error: ${event.message || "unknown"}`, "error");
    log(`worker error: ${event.message || "unknown"}`, "error");
  };
  const boot: ToWorker = { type: "boot", wasmUrl: engineWasmUrl };
  worker.postMessage(boot);
  log("worker 1 spawned");
}

/**
 * Ask for one game. One in flight, one pending, latest wins: the worker runs
 * `gen_game` + `solve` synchronously inside its message handler, so a second request
 * cannot interrupt the first.
 */
function requestGame(input: GameInput): void {
  if (!worker) return;
  if (!booted) return; // the `ready` handler solves whatever `game` names by then
  if (inFlight) {
    queued = input;
    return;
  }
  inFlight = true;
  pending = nextRequestId;
  nextRequestId += 1;
  // §2.3: the interactive solve is traced, at the band the build ships. The band
  // bounds the records and never the search, so this changes no figure.
  const message: ToWorker = { type: "solve", requestId: pending, band: DEFAULT_BEAM, ...input };
  say(statusLine, `${describeGame(input)} — solving…`);
  outcome = "solving";
  elapsedMs = 0;
  paintOutcome();
  app.dataset.solving = "true";
  log(`solve(seed ${input.seed}, goal ${input.goal}, ${input.pieces} pieces, beam ${input.beamWidth})`);
  worker.postMessage(message);
}

function drain(): void {
  inFlight = false;
  if (queued === null) return;
  const next = queued;
  queued = null;
  requestGame(next);
}

function receive(message: FromWorker): void {
  switch (message.type) {
    case "ready": {
      bootSlot.textContent = `worker 1 · abi ${message.abiVersion} · scratch at 0x${message.scratchOffset.toString(16)}`;
      log(`abi ${ABI} · wasm ready · scratch at 0x${message.scratchOffset.toString(16)}`);
      booted = true;
      requestGame(game);
      return;
    }
    case "game": {
      if (message.requestId !== pending) return; // a stale answer
      drain();
      show(message);
      return;
    }
    case "error": {
      if (message.requestId !== 0 && message.requestId !== pending) return;
      const fatal = message.code === "abi-mismatch";
      say(statusLine, `${fatal ? "fatal" : "error"}: ${message.message}`, "error");
      outcomeLine.textContent = "";
      log(`${fatal ? "fatal" : "error"}: ${message.message}`, "error");
      if (!fatal) {
        console.warn(`[tetris-lab] ${message.code}: ${message.message}`);
        if (message.code === "wasm-trap") spawn();
        else drain();
      }
      return;
    }
    default:
      return;
  }
}

// ── the game: clamp, write the URL, ask for the solve ───────────────────────
function applyGame(candidate: GameInput): void {
  const next = clampGame(candidate, BOUNDS);
  const reason = reSolveReason(next, game);
  playgroundRoot.dataset.reason = reason;
  if (reason === "none") return;
  game = next;
  paintIdentity();
  writeUrl(next);
  requestGame(next);
}

/**
 * `replaceState`, never `pushState`: one entry per commit means a slider drag would
 * otherwise append ninety session-history entries, and Back would walk the slider
 * tick by tick instead of leaving the Lab.
 */
function writeUrl(input: GameInput): void {
  const search = serialiseGame(input, BOUNDS);
  if (location.search === search) return;
  try {
    const url = new URL(location.href);
    url.search = search;
    history.replaceState(null, "", url);
  } catch (error) {
    console.warn(`[tetris-lab] could not write the URL: ${String(error)}`);
  }
}

// ── what the page says about the game ──────────────────────────────────────

/** The Engine panel's width figure and the Playground head's seed line. */
function paintIdentity(): void {
  e1.set(String(game.beamWidth), `of ${BOUNDS.widthCap}`);
}

/** The outcome line: a word, the lines against the goal, and the solve time. */
function paintOutcome(): void {
  const goal = shown?.input.goal ?? game.goal;
  if (outcome === "solving") {
    outcomeLine.dataset.outcome = "solving";
    outcomeLine.textContent = `Solving seed ${game.seed}…`;
    app.dataset.solving = "true";
    return;
  }
  const lines = shown?.outcome.lines ?? 0;
  const won = shown?.outcome.won ?? false;
  outcomeLine.dataset.outcome = won ? "solved" : "lost";
  outcomeLine.textContent = won
    ? `Win · ${lines} of ${goal} lines · ${elapsedMs.toFixed(1)} ms`
    : `Loss · reached ${lines} of ${goal} lines · ${elapsedMs.toFixed(1)} ms`;
  app.dataset.solving = "false";
}

/** The status bar: Evaluations, the one solve number shown nowhere else. */
function paintStatus(): void {
  const evaluations = shown?.outcome.evaluations ?? 0;
  s1.set(evaluations.toLocaleString("en-US"), "per solve");
  i9.set(traced ? `${traced.layerCount} layers` : "no layers yet");
}

// ── the readout, the table, the band ───────────────────────────────────────

/** The Layer on screen: the Moves played, or the failing Layer of a lost game. */
function layerOnScreen(): number {
  if (!traced || traced.layerCount === 0) return 0;
  const lost = shown !== null && shown.moves.length === 0;
  const k = lost ? traced.layerCount - 1 : (playback ? playback.k : 0);
  return Math.min(traced.layerCount - 1, Math.max(0, k));
}

/** The Board at the Move on screen: `board0` at Move 0, `postClear` after Move k. */
function boardAt(k: number): Field {
  if (playback && k > 0) {
    const step = playback.timeline.steps[k - 1];
    if (step) return step.postClear;
  }
  return shownBoard;
}

/** The ten-column profile, as ten bars with the tallest column as the scale. */
function paintColumns(k: number): void {
  const profile = columnProfile(boardAt(k));
  const scale = profileScale(profile);
  cols.replaceChildren();
  for (const entry of profile) {
    const column = el("div", "c");
    column.dataset.tip = profileTip(entry.column, entry);
    const bar = el("b");
    bar.style.height = `${Math.round((entry.height / scale) * 100)}%`;
    if (entry.holes > 0) {
      const holes = el("u");
      column.append(holes);
    }
    column.prepend(bar);
    cols.append(column);
  }
  caps.replaceChildren();
  for (let index = 0; index < W; index += 1) {
    const cap = el("span");
    cap.textContent = String(index);
    caps.append(cap);
  }
  c6.num.textContent = `${W} columns`;
  c6.unit.textContent = `tallest ${scale} rows`;
}

/** This Placement: the words the readout's second section prints. */
function paintPlacement(k: number): void {
  const step = playback && k > 0 ? playback.timeline.steps[k - 1] : null;
  if (!step) {
    c7.set("Move 0 is the starting Board", "");
    c8.set("not placed", "");
    c9.set("not placed", "");
    c10.set("0");
    c11.set("0");
    return;
  }
  c7.set(`${letterOf(step.piece)} · rotation ${step.rotation}`, "");
  c8.set(String(step.column));
  c9.set(String(Math.max(...step.restCells.map((cell) => cell.row))));
  c10.set(String(step.cleared.length));
  c11.set(String(step.linesAfter));
}

/** Next in Sequence: four glyphs, and how many are left after them. */
function paintNext(k: number): void {
  const sequence = shown?.sequence ?? "";
  nextSeq.replaceChildren();
  const letters: string[] = [];
  for (let at = k; at < Math.min(k + 4, sequence.length); at += 1) {
    const letter = sequence[at];
    letters.push(letter);
    const glyph = el("span");
    glyph.append(pieceGlyph(letter, 9));
    const steps = at - k + 1;
    glyph.dataset.tip = `${letter} — ${steps === 1 ? "next" : `${steps} ahead`}.`;
    nextSeq.append(glyph);
  }
  if (letters.length === 0) {
    const none = el("span", "chip");
    none.textContent = "the Sequence is spent";
    nextSeq.append(none);
  }
  c12.num.textContent = `${Math.min(4, Math.max(0, sequence.length - k))} of ${sequence.length}`;
  c12.unit.textContent = "pieces";
}

/** Per-layer table: one row per layer, every column the trace's own. */
function paintTable(currentLayer: number): void {
  if (!traced || !shown) {
    layerRowsBody.replaceChildren();
    const row = el("tr");
    const cell = el("td", "empty");
    cell.colSpan = 5;
    cell.textContent = "Fills at the first solve.";
    row.append(cell);
    layerRowsBody.append(row);
    c5.set("no solve yet");
    return;
  }
  const rows = layerRows(traced, shown.sequence);
  const onScreen = rows.filter((entry) => entry.layer === currentLayer);
  c5.set(String(onScreen[0]?.best ?? rows[rows.length - 1]?.best ?? ""), "score");
  const built = rows.map((entry) => {
    const row = el("tr");
    if (entry.layer === currentLayer) row.dataset.current = "true";
    row.dataset.tip = `Layer ${entry.layer} · ${entry.piece} · best score ${entry.best} · ${entry.lines} Lines cleared · ${entry.kept} states kept`;
    const layer = el("td");
    layer.textContent = String(entry.layer).padStart(2, "0");
    const piece = el("td", "piece");
    piece.append(pieceGlyph(entry.piece, 7));
    const best = el("td");
    best.textContent = String(entry.best);
    const lines = el("td");
    lines.textContent = String(entry.lines);
    const kept = el("td");
    kept.textContent = String(entry.kept);
    row.append(layer, piece, best, lines, kept);
    return row;
  });
  layerRowsBody.replaceChildren(...built);
}

/** The band: ticket 11's model and canvas, with the panel sized to the notes' rule. */
function showBand(message: GameMessage): void {
  if (!message.trace || message.trace.length === 0) return;
  const trace = decodeTrace(message.trace, message.band ?? DEFAULT_BEAM);
  traced = trace;
  const model = buildBand(trace, { sequence: message.sequence, shippedBeam: DEFAULT_BEAM });
  bandModel = model;
  bandView.resize();
  bandView.draw(model);
  say(bandLabel, model.label);
  const caption =
    model.clipped > 0
      ? `${model.caption} · ${model.clipped} lineage points have a parent the band did not keep`
      : model.caption;
  say(bandCaption, caption);
  bandCaption.dataset.tip = `${caption}. The band is the beam: one row per Layer, exactly as many states per row as survived it, each placed by its score on one shared axis.`;
  i2.set(`${model.records} of ${model.records} states · ${model.layerCount} Layers`, "");
  i5.set(`${model.rows[0]?.best ?? 0} in Layer 0`, "score");
  const items = countLines(model).map((text, layer) => {
    const item = el("li");
    item.textContent = text;
    item.dataset.tip = `Layer ${layer} — ${text.replace(/^L\d+ /, "")}`;
    if (model.lineage.some((point) => point.layer === layer)) item.dataset.lineage = "true";
    return item;
  });
  bandCounts.replaceChildren(...items);
  showTree(trace, message.sequence);
}

/**
 * The bounded tree's half: the same decoded trace the band just read, handed to
 * the drill-down. Nothing here solves, counts or re-derives — the tree is a
 * *projection* of a recording the engine already made (§2.3), so it costs one pass
 * over records that are already in memory.
 */
function showTree(trace: Trace | null, sequence: string | undefined): void {
  traceLineage = trace === null ? [] : winningLineage(trace).map((point) => ({ layer: point.layer, rank: point.rank }));
  treePanel.setTrace(trace, sequence, boardFor);
  syncTreeNote();
}

/** The selected state's Board, when this page holds it; `null` when it does not. */
function boardFor(layer: number, rank: number): ArrayLike<number> | null {
  if (layer === 0 && rank === 0) return shownBoard;
  if (playback === null) return null;
  const onPath = traceLineage[layer];
  if (onPath === undefined || onPath.rank !== rank) return null;
  if (layer > playback.timeline.steps.length) return null;
  return reconstruct(playback.timeline, layer);
}

/**
 * The tree's three head figures, from the panel's own model — never from a copy.
 * While the tree is closed there is no model to measure, so I7 says "not drawn yet"
 * rather than a zero that would read as "the tree drew nothing".
 */
function syncTreeNote(): void {
  const model = treePanel.treeModel;
  const selection = treePanel.selectionRef;
  i6.set(String(treePanel.treeBound), "per Layer");
  if (model === null) {
    i7.set("not drawn yet", "nodes");
    i8.set("the tree is closed", "Layers");
  } else {
    i7.set(String(model.nodes.length), "nodes");
    i8.set(`${model.window.from + 1} of ${model.window.total}`, "Layers");
  }
  i11.set(selection === null ? "no state selected yet" : `Layer ${selection.layer} · rank ${selection.rank}`, "score");
}

/**
 * The board's row numbers, one per row: the gutter is a grid of twenty `--cell`
 * rows, padded down to where the board's first row starts, so "0" sits beside row 0
 * whatever whole-pixel cell the canvas settled on.
 */
function alignGutter(): void {
  const cell = Number(canvas.dataset.cell ?? 0);
  const top = Number(canvas.dataset.originY ?? 0);
  gutter.style.setProperty("--cell", `${cell}px`);
  gutter.style.paddingTop = `${top}px`;
}

/** Size the board to its box and line the row numbers up with it. */
function resizeBoard(): void {
  view.resize();
  alignGutter();
}

/** Redraw the inspector's canvases at their current boxes (after a view or mode change). */
function relayoutInspector(): void {
  bandView.resize();
  bandView.draw(bandModel);
  treePanel.draw();
  syncTreeNote();
}

// ── painting the whole page from one state ─────────────────────────────────
function paintAll(state?: PlaybackStatus): void {
  const layer = layerOnScreen();
  const k = state ? Math.min(state.played, state.total) : (playback ? playback.k : 0);
  paintIdentity();
  paintOutcome();
  paintStatus();
  paintColumns(k);
  paintPlacement(k);
  paintNext(k);
  paintTable(layer);
  c1.set(`${layer} of ${traced?.layerCount ?? 0}`, "");
  const step = playback && k > 0 ? playback.timeline.steps[k - 1] : null;
  let nextLetter = "?";
  if (step) nextLetter = letterOf(step.piece);
  else if (shown) nextLetter = letterOf((shown.moves[0] >>> 16) & 0xff);
  pieceNow.replaceChildren(
    svgElement(svgIcon("brand-mark")),
    word(step ? "placed" : "next"),
    pieceGlyph(nextLetter, 5),
    word(nextLetter),
  );
  pieceNow.dataset.tip = `The piece the move on screen placed.`;
}

/** One labelled chip inside the piece-now slot. */
function word(text: string): HTMLElement {
  const span = el("span", "word");
  span.textContent = text;
  return span;
}

// ── showing a game ─────────────────────────────────────────────────────────

function paintStrip(k: number, total: number): void {
  if (painted.total !== total) {
    strip.replaceChildren();
    const mode = stripMode(total);
    for (let index = 0; index < total; index += 1) {
      const cell = el("i");
      const word = shown?.moves[index];
      const clears = word === undefined ? false : Boolean(clearedMoveFlag[index]);
      if (clears) cell.dataset.clears = "true";
      cell.dataset.tip =
        word === undefined
          ? `Move ${index + 1}`
          : `Move ${index + 1} · ${letterOf((word >>> 16) & 0xff)} → column ${word & 0xff}${clears ? " · cleared rows" : ""}`;
      if (mode === "tick") {
        cell.dataset.tick = "true";
      } else if (word !== undefined) {
        // Glyph mode: the cell is wide enough to say which piece the move placed.
        // A tick at 60+ moves is the same information squeezed past legibility.
        cell.append(pieceGlyph(letterOf((word >>> 16) & 0xff), 5));
      }
      strip.append(cell);
    }
    strip.dataset.mode = mode;
    strip.setAttribute("aria-valuemax", String(total));
    painted = { k: -1, total };
  }
  strip.setAttribute("aria-valuenow", String(k));
  strip.setAttribute("aria-valuetext", `Move ${k} of ${total}`);
  m2.set(String(k), `of ${total} Moves`);
  if (painted.k === k) return;
  const cells = strip.children;
  for (let index = 0; index < cells.length; index += 1) {
    const cell = cells[index];
    const played = index < k;
    if (cell.getAttribute("data-played") !== String(played)) cell.setAttribute("data-played", String(played));
    const current = index === k - 1;
    if (cell.getAttribute("data-current") !== String(current)) cell.setAttribute("data-current", String(current));
  }
  const width = strip.clientWidth;
  caret.style.left = `${moveToX(k, width, total) - 1}px`;
  painted.k = k;
}

/**
 * A game the search could not win is still an outcome (ticket bullet 8).
 *
 * The loss path returns `status 0`, `won false`, the Lines the search reached and no
 * Move list at all, so there is nothing to animate and the strip has nothing to seek.
 * The page says what happened in words, draws the Board the game was given, and
 * disables the transport rather than leaving a play button that plays nothing.
 */
function showLoss(message: GameMessage): void {
  const words = `No moves to replay: the search stopped at ${message.outcome.lines} lines.`;
  for (const button of [playButton, prevButton, nextButton, restartButton]) button.disabled = true;
  setPlayButton("play", false);
  newGameStrip(words);
}

/** The strip, when there is nothing to play: one labelled cell instead of a hole. */
function newGameStrip(words: string): void {
  strip.replaceChildren();
  const cell = el("i");
  cell.dataset.empty = "true";
  cell.textContent = "No moves to replay";
  cell.dataset.tip = words;
  strip.dataset.mode = "none";
  strip.append(cell);
  strip.setAttribute("aria-valuemax", "0");
  strip.setAttribute("aria-valuenow", "0");
  strip.setAttribute("aria-valuetext", "No Moves to replay");
  caret.style.left = "0px";
  painted = { k: 0, total: 0 };
  clearedMoveFlag = [];
  m2.set("0", "of 0 moves");
}

function setPlayButton(word: "play" | "pause" | "replay", pressed: boolean): void {
  playWord.textContent = word;
  mountIcon(playIcon, word === "pause" ? "pause" : "play", 10);
  playButton.setAttribute("aria-pressed", String(pressed));
  const aria = word === "replay" ? "Replay from the start" : (word === "pause" ? "Pause" : "Play");
  playButton.setAttribute("aria-label", aria);
}

function show(message: GameMessage): void {
  const { input } = message;
  shown = message;
  elapsedMs = message.elapsedMs;
  outcome = message.outcome.won ? "solved" : "lost";
  bootSlot.textContent = `worker 1 · abi ${ABI} · ${message.sequence.length} pieces`;
  log(
    `${outcome} · ${message.outcome.lines} of ${input.goal} Lines · ${message.outcome.moveCount} Moves · ${message.outcome.evaluations.toLocaleString("en-US")} Evaluations · ${message.elapsedMs.toFixed(1)} ms`,
  );
  say(statusLine, describeGame(input));

  if (playback) playback.stop();
  playback = null;
  shownBoard = fieldFrom(message.board);
  resizeBoard();
  view.draw(shownBoard);
  traced = null;
  showBand(message);

  if (message.moves.length === 0) {
    paintAll();
    showLoss(message);
    return;
  }

  for (const button of [playButton, prevButton, nextButton, restartButton]) button.disabled = false;
  playback = new Playback(view, message, {
    speed,
    onStatus(state) {
      paintStrip(Math.min(state.played, state.total), state.total);
      const word = state.atEnd ? "replay" : (state.playing ? "pause" : "play");
      setPlayButton(word, state.playing);
      paintStatus();
      const layer = layerOnScreen();
      paintColumns(Math.min(state.played, state.total));
      paintPlacement(Math.min(state.played, state.total));
      paintNext(Math.min(state.played, state.total));
      paintTable(layer);
      c1.set(`${layer} of ${traced?.layerCount ?? 0}`, "");
    },
  });

  clearedMoveFlag = playback.timeline.steps.map((step) => step.cleared.length > 0);
  painted = { k: -1, total: -1 };
  // Announce Move 0 now rather than on the first frame: otherwise the progress line
  // keeps the previous game's words for as long as this one takes to start.
  playback.seek(0);
  paintAll();

  // Show the starting position before anything moves: the solve is about a real
  // board, and a visitor should see it.
  if (startTimer !== 0) clearTimeout(startTimer);
  startTimer = globalThis.setTimeout(() => {
    startTimer = 0;
    playback?.play();
  }, 400);
}

// ── controls ───────────────────────────────────────────────────────────────
function currentK(): number {
  return playback ? playback.k : 0;
}

function scrubTo(k: number): void {
  if (!playback) return;
  playback.seek(k);
}

function setSpeed(next: number): void {
  speed = next;
  if (playback) playback.setSpeed(speed);
  speedSlider.value = String(mpsToSlider(speed));
  const label = `${speed.toFixed(2)} Moves/s · ${(msPerMove(speed) / 1000).toFixed(2)} s per Move`;
  // Two elements, one figure: the numeral is tabular and the unit is the words, so a
  // speed change never reflows the transport (§4.2's rule about numbers).
  speedFigureNum.textContent = speed.toFixed(2);
  speedFigureUnit.textContent = `Moves/s · ${(msPerMove(speed) / 1000).toFixed(2)} s per Move`;
  speedReadout.setAttribute("aria-label", label);
  speedSlider.setAttribute("aria-valuetext", label);
}

/** One key or one button, one effect: the map and the controls share this. */
function run(action: Action): void {
  switch (action.type) {
    case "keys":
      setSheet(sheet.dataset.open !== "true");
      return;
    case "close":
      // Escape also clears the tree's selection: one key, "nothing is pinned any
      // more" (ticket 12's rule).
      treePanel.clearSelection();
      // `?` opens the sheet *inside* the drawer, so it is one gesture and one Escape
      // undoes it; a drawer opened on its own closes on its own.
      if (sheet.dataset.open === "true") {
        setSheet(false);
        setDrawer(false);
      } else {
        setDrawer(false);
      }
      return;
    case "explainer":
      setDrawer(drawer.dataset.open !== "true");
      return;
    case "speed": {
      const stop = Math.log(SPEED_STOP) / Math.log(MPS_MAX / MPS_MIN);
      setSpeed(sliderToMps(Math.min(1, Math.max(0, mpsToSlider(speed) + action.direction * stop))));
      return;
    }
    default:
      break;
  }
  if (!playback) return;
  switch (action.type) {
    case "toggle":
      playback.toggle();
      return;
    case "nudge":
      scrubTo(currentK() + action.delta);
      return;
    case "edge":
      scrubTo(action.to === "start" ? 0 : playback.timeline.steps.length);
      return;
    case "restart":
      playback.restart();
      return;
    default:
      return;
  }
}

/**
 * The Explainer drawer.
 *
 * It opens over the page rather than pushing it: a no-scroll page has nowhere else to
 * put the Explainer's words, and a drawer that reflowed the grid would move the Board —
 * which is the one thing §2 says must not happen.
 *
 * Focus moves into the drawer on open and back to whatever opened it on close, so
 * `How it works`, `E`, the close button and `Esc` are one state (`data-open`) with one
 * owner — and `E` still closes a drawer the button opened, because `resolveKey` lets
 * `E` past a focused button (the button owns Space and the arrows, not the drawer's
 * keys).
 */
let drawerReturn: HTMLElement | null = null;

function setDrawer(open: boolean): void {
  const wasOpen = drawer.dataset.open === "true";
  if (open && !wasOpen) {
    const active = document.activeElement;
    drawerReturn = active instanceof HTMLElement && active !== document.body ? active : null;
  }
  drawer.dataset.open = String(open);
  drawer.setAttribute("aria-hidden", String(!open));
  scrim.dataset.open = String(open);
  // `#drawer` carries `tabindex="-1"`, so the panel — not its close button — takes
  // focus and the screen reader announces the title, not the first control. On close,
  // the keyboard sheet goes with the panel and focus returns to whatever opened it.
  if (open) {
    drawer.focus();
    return;
  }
  setSheet(false);
  const back = drawerReturn;
  drawerReturn = null;
  if (back?.isConnected) back.focus();
}

function setSheet(open: boolean): void {
  sheet.dataset.open = String(open);
  if (open) {
    if (drawer.dataset.open !== "true") setDrawer(true);
    sheet.setAttribute("tabindex", "-1");
    sheet.focus();
  }
}

// ── the keyboard map, in words ─────────────────────────────────────────────
for (const { keys: chord, does } of KEY_MAP) {
  const term = el("dt");
  for (const key of chord.split(" ")) {
    const chip = el("span", "kbd");
    chip.textContent = key;
    term.append(" ", chip);
  }
  const detail = el("dd");
  detail.textContent = does;
  sheetList.append(term, detail);
}

// Ticket 16's prose: nine sections, every figure resolved from the artifact that
// owns it, mounted once before the first paint. The sheet above is a sibling and is
// never replaced.
mountExplainer(explainerBody);

/* ---- the move strip is the seek bar ------------------------------------ */
strip.addEventListener("pointerdown", (event) => {
  if (!playback) return;
  event.preventDefault();
  strip.focus();
  strip.setPointerCapture(event.pointerId);
  dragging = true;
  resumeAfterDrag = playback.playing;
  const rect = strip.getBoundingClientRect();
  scrubTo(xToMove(event.clientX - rect.left, rect.width, playback.timeline.steps.length));
});

strip.addEventListener("pointermove", (event) => {
  if (!dragging || !playback) return;
  const rect = strip.getBoundingClientRect();
  scrubTo(xToMove(event.clientX - rect.left, rect.width, playback.timeline.steps.length));
});

const endDrag = (event: PointerEvent): void => {
  if (!dragging || !playback) return;
  dragging = false;
  if (strip.hasPointerCapture(event.pointerId)) strip.releasePointerCapture(event.pointerId);
  if (resumeAfterDrag) playback.play();
  resumeAfterDrag = false;
};

strip.addEventListener("pointerup", endDrag);
strip.addEventListener("pointercancel", endDrag);

/* ---- the transport ---------------------------------------------------- */
playButton.addEventListener("click", () => playback?.toggle());
prevButton.addEventListener("click", () => run({ type: "nudge", delta: -1 }));
nextButton.addEventListener("click", () => run({ type: "nudge", delta: 1 }));
restartButton.addEventListener("click", () => playback?.restart());
speedSlider.addEventListener("input", () => setSpeed(sliderToMps(Number(speedSlider.value))));
solveButton.addEventListener("click", () => requestGame(game));

/* ---- the drawer, the Expert toggle and the tree ------------------------ */
// The two view buttons are ticket 12's; these listeners only keep the head's figures
// in step, and they run after the panel's own because they are bound later.
for (const button of inspectorViews.querySelectorAll("button")) {
  button.addEventListener("click", () => relayoutInspector());
}
treeBoundSlider.addEventListener("input", () => syncTreeNote());
howButton.addEventListener("click", () => setDrawer(drawer.dataset.open !== "true"));
drawerClose.addEventListener("click", () => setDrawer(false));
scrim.addEventListener("click", () => setDrawer(false));
sheet.addEventListener("click", (event) => {
  if (event.target === sheet) setSheet(false);
});
expertButton.addEventListener("click", () => {
  const open = expertButton.getAttribute("aria-expanded") !== "true";
  expertButton.setAttribute("aria-expanded", String(open));
  engineWeights.hidden = !open;
});


/* ---- the keyboard ----------------------------------------------------- */
globalThis.addEventListener("keydown", (event) => {
  const action = resolveKey(event);
  if (!action) return;
  // Every key in the map is one the browser would otherwise use, so take it
  // deliberately. `overflow: hidden` makes a missing `preventDefault()` invisible.
  event.preventDefault();
  run(action);
});

// A hidden tab's animation frames are unpredictable, so pause instead of trusting
// them: the clock's stall guard covers the rest.
document.addEventListener("visibilitychange", () => {
  if (document.hidden && playback?.playing) playback.pause();
});

/* ---- resize ----------------------------------------------------------- */
globalThis.addEventListener("resize", () => {
  resizeBoard();
  relayoutInspector();
  redrawBoard();
});

/** Repaint the Board and the strip at their current size, from the replay or the loose Board. */
function redrawBoard(): void {
  if (playback) {
    playback.draw();
    painted = { k: -1, total: painted.total };
    paintStrip(currentK(), painted.total);
  } else {
    view.draw(shownBoard);
  }
}

/* ---- the two modes: watch one solve, or measure many ------------------- */
type Mode = "watch" | "measure";

/** The mode a tab button stands for; anything but "measure" is Watch. */
function modeOf(button: HTMLElement): Mode {
  return button.dataset.mode === "measure" ? "measure" : "watch";
}

/**
 * Measure takes the centre and the inspector's width for the Run; Watch gives them
 * back. The board and the band were drawn at their old boxes, so returning to Watch
 * resizes and redraws them.
 */
function setMode(mode: Mode): void {
  if (app.dataset.mode === mode) return;
  app.dataset.mode = mode;
  panelRun.hidden = mode !== "measure";
  for (const button of modeButtons) {
    button.setAttribute("aria-selected", String(button.dataset.mode === mode));
    button.tabIndex = button.dataset.mode === mode ? 0 : -1;
  }
  if (mode === "measure") {
    if (playback?.playing) playback.pause();
    return;
  }
  resizeBoard();
  relayoutInspector();
  redrawBoard();
}

for (const button of modeButtons) {
  button.addEventListener("click", () => setMode(modeOf(button)));
  // A tablist: the arrow keys move between the two tabs, and nothing else sees them.
  button.addEventListener("keydown", (event) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    event.stopPropagation();
    const other = modeButtons.find((candidate) => candidate !== button);
    if (!other) return;
    other.focus();
    setMode(modeOf(other));
  });
}

/* ---- the tour ---------------------------------------------------------- */
// Every stop except the last points at the Watch view, so the tour puts it on
// screen. A first visit opens it (see the end of this file); closing it by any
// route — Skip, Done or Escape — is remembered, and the toolbar reopens it.
const tour = new Tour({
  prepare: (stop) => {
    if (stop.anchor !== "#mode-measure") setMode("watch");
    if (drawer.dataset.open === "true") setDrawer(false);
  },
});
tourOpenButton.addEventListener("click", () => tour.open());
tourStartButton.addEventListener("click", () => tour.open());

/* ---- the Playground and the load path --------------------------------- */
const playground = new Playground(playgroundRoot, {
  bounds: BOUNDS,
  onSolve: applyGame,
});

/**
 * The load path (§2.2): parse the URL, show it in the controls, and solve it once.
 * The URL is **not** rewritten here — a visitor's unknown keys are theirs until they
 * change something.
 */
game = parseGame(location.search, BOUNDS);
playground.fill(game);
playgroundRoot.dataset.reason = "none";
paintIdentity();

// ── first paint: the skeleton, filled with what is already true ────────────
installTooltips();
adoptTitles();
setSpeed(speed);
sheet.dataset.open = "false";
drawer.dataset.open = "false";
scrim.dataset.open = "false";
app.dataset.mode = "watch";
resizeBoard();
view.draw(shownBoard);
paintAll();
log("page loaded");
spawn();
// A browser that has never closed the tour gets it now, over the first solve.
if (!tourSeen()) tour.open();
