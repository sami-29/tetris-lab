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
import { buildBand, countLines, decodeTrace, layerSlice, type BandModel } from "./ui/band-model.ts";
import type { Trace } from "./ui/trace.ts";
import { Playback, type PlaybackStatus } from "./ui/playback.ts";
import { Playground } from "./ui/playground.ts";
import { letterOf } from "./ui/pieces.ts";
import { MPS_DEFAULT, MPS_MAX, MPS_MIN, mpsToSlider, msPerMove, sliderToMps } from "./ui/phases.ts";
import { KEY_MAP, SPEED_STOP, resolveKey, type Action } from "./ui/keys.ts";
import { moveToX, stripMode, xToMove } from "./ui/seek.ts";
import { createFigure, adoptFigure, type FigureId, type FigureView } from "./ui/figure.ts";
import { pieceGlyph, svgIcon, type IconName } from "./ui/icons.ts";
import { adoptTitles, installTooltips } from "./ui/tooltip.ts";
import { columnProfile, profileScale, profileTip } from "./ui/readout.ts";
import { layerRows, logStamp, sparkline } from "./ui/inspector.ts";

/** §4's handshake number, echoed in the status bar's boot slot. */
const ABI = 2;

/**
 * The two numbers the page hangs on, read at build time from the committed width
 * curve through `./config/width.ts` — the one module allowed to name them.
 */
const BOUNDS: Bounds = { defaultBeam: DEFAULT_BEAM, widthCap: WIDTH_CAP };

/** Milliseconds since this page loaded: the log's and `elapsed`'s one clock. */
const bootAt = performance.now();

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
const progressLine = must<HTMLElement>("#progress");
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
const treeButton = must<HTMLButtonElement>("#tree-toggle");
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
const playgroundRoot = must<HTMLElement>("#playground");
const presetName = must<HTMLElement>("#preset-name");
const bandCanvas = must<HTMLCanvasElement>("#band");
const bandPanel = must<HTMLElement>("#band-panel");
const bandLabel = must<HTMLElement>("#band-label");
const bandCaption = must<HTMLElement>("#band-caption");
const bandCounts = must<HTMLElement>("#band-counts");
const bandLegend = must<HTMLElement>("#band-legend");
const treePanel = must<HTMLElement>("#tree-panel");
const treeNote = must<HTMLElement>("#tree-note");
const treeBody = must<HTMLElement>("#tree");
const layerCountSlot = must<HTMLElement>("#layer-count");
const layerRowsBody = must<HTMLElement>("#layer-rows");
const logList = must<HTMLElement>("#log");
const logState = must<HTMLElement>("#log-state");
const layerLine = must<HTMLElement>("#layer-line");
const pieceNow = must<HTMLElement>("#piece-now");
const gutter = must<HTMLElement>("#gutter");
const roScore = must<HTMLElement>("#ro-score");
const roColumns = must<HTMLElement>("#ro-columns");
const roPlacement = must<HTMLElement>("#ro-placement");
const roNext = must<HTMLElement>("#ro-next");
const roLayer = must<HTMLElement>("#ro-layer");
const sparkHost = must<HTMLElement>("#spark");
const statusFigures = must<HTMLElement>("#status-figures");
const engineFigures = must<HTMLElement>("#engine-figures");
const engineWeights = must<HTMLElement>("#engine-weights");
const runRows = must<HTMLElement>("#run-rows");
const runState = must<HTMLElement>("#run-state");
const referenceLabels = must<HTMLElement>("#reference-labels");
const moveFigureHost = must<HTMLElement>("#move-figure");
const inspector = must<HTMLElement>("#inspector");

const view = new BoardView(canvas);
const bandView = new BandView(bandCanvas);

// ── the shell: icons, figures, tooltips ──────────────────────────────────────

/** Every `[data-icon]` placeholder gets its hand-made SVG. */
for (const host of document.querySelectorAll<HTMLElement>("[data-icon]")) {
  host.innerHTML = svgIcon(host.dataset.icon as IconName);
}
document.getElementById("play-icon")!.innerHTML = svgIcon("play", 10);

/** The figures, by id, so an update is one lookup and one `set`. */
const FI: Partial<Record<FigureId, FigureView>> = {};

function fig(id: FigureId, host: HTMLElement, initial?: string): FigureView {
  const built = createFigure(id, initial);
  host.append(built.el);
  FI[id] = built;
  return built;
}

/** Adopt a figure the markup already carries (the toolbar's four fields). */
function adopt(id: FigureId): FigureView {
  const found = adoptFigure(id);
  if (!found) throw new Error(`index.html is missing the ${id} figure`);
  FI[id] = found;
  return found;
}

const t1 = adopt("t1");
const t2 = adopt("t2");
const t3 = adopt("t3");
const t4 = adopt("t4");

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

// The Run panel: the rows exist so the panel is never empty, and every value slot
// says what would fill it. Ticket 15 makes them real; nothing here guesses.
for (const [id, label] of [
  ["r1", "Win rate"],
  ["r2", "Games won"],
  ["r3", "Mean Lines on failure"],
  ["r4", "Median Lines on failure"],
  ["r5", "Evaluations per Solve"],
  ["r6", "ms per Solve"],
  ["r7", "Total"],
] as const) {
  const row = el("tr");
  const name = el("td");
  name.textContent = label;
  name.title = "";
  const cell = el("td");
  const figure = fig(id, cell, "not measured");
  figure.el.classList.add("rtable-fig", "nolabel");
  row.append(name, cell);
  runRows.append(row);
}
referenceLabels.replaceChildren();
const r8 = fig("r8", referenceLabels);
r8.set("no Reference row is committed yet", "");
referenceLabels.dataset.tip = r8.el.dataset.tip ?? "";
runState.replaceChildren();
const r9 = fig("r9", runState);
r9.el.classList.add("nolabel");
r9.set("no Run in this tab yet", "");

// The status bar's four figures, in the glossary's words (§2.6, story 9).
const s1 = fig("s1", statusFigures, "0");
const s2 = fig("s2", statusFigures, "0 of 0");
const s3 = fig("s3", statusFigures, "0 of 8");
const s4 = fig("s4", statusFigures, "0.0");

// The move figure and speed, in the transport.
const m1 = fig("m1", moveFigureHost, "0");
const m2 = fig("m2", moveFigureHost, "0");
m2.el.classList.add("nolabel");

// The board header's line: the Layer on screen, and the Goal it is being solved to.
layerLine.replaceChildren();
const c1 = fig("c1", layerLine, "Layer 0 of 0");
const c2 = fig("c2", layerLine, String(BOUNDS.defaultBeam > 0 ? 8 : 8));

// The readout, section by section (notes/14 §2.3).
const c5 = fig("c5", roScore, "awaiting the first Solve");
const c6 = createFigure("c6");
c6.el.classList.add("fig-picture");
const cols = el("div", "cols");
const caps = el("div", "caps");
c6.el.classList.add("nolabel");
c6.el.append(cols, caps);
roColumns.append(c6.el);
FI.c6 = c6;
const c7 = fig("c7", roPlacement, "Move 0 is the starting Board");
const c8 = fig("c8", roPlacement, "not placed");
const c9 = fig("c9", roPlacement, "not placed");
const c10 = fig("c10", roPlacement, "0");
const c11 = fig("c11", roPlacement, "0");
const c12 = createFigure("c12");
c12.el.classList.add("fig-picture");
const nextSeq = el("span", "seq");
const nextWords = el("span", "chip");
c12.el.classList.add("nolabel");
c12.el.append(nextSeq, nextWords);
roNext.append(c12.el);
FI.c12 = c12;
const c13 = fig("c13", roLayer, "not measured");
const c15 = fig("c15", roLayer, "not measured");
const c16 = fig("c16", roLayer, "not measured");
const c17 = fig("c17", roLayer, "not measured");
const c18 = createFigure("c18");
c18.el.classList.add("fig-picture");
const sparkHostSvg = el("span", "spark");
c18.el.classList.add("nolabel");
c18.el.append(sparkHostSvg);
sparkHost.append(c18.el);
FI.c18 = c18;

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
treeBody.replaceChildren();
fig("i11", treeBody, "no state selected yet").el.classList.add("nolabel");

// The tree's head and body: ticket 12's plot mounts at `#tree`, and until it does the
// pane says what it will draw rather than showing an empty box.
treeNote.replaceChildren();
const i6 = fig("i6", treeNote, "24");
const i7 = fig("i7", treeNote, "0");
const i8 = fig("i8", treeNote, "no Layer yet");
for (const view of [i6, i7, i8]) view.el.classList.add("nolabel");
treeBody.append(el("p", "note"));

// The per-layer table's head count and the log's line count.
layerCountSlot.replaceChildren();
logState.replaceChildren();
const i9 = fig("i9", layerCountSlot, "0 of 0 Layers");
const i10 = fig("i10", logState, "0 lines");
i9.el.classList.add("nolabel");
i10.el.classList.add("nolabel");

// ── the log ─────────────────────────────────────────────────────────────────

/** One line of the log: a stamp since load, then the words. */
function log(words: string, tone: "plain" | "error" = "plain"): void {
  const item = el("li");
  item.dataset.tone = tone;
  const time = el("time");
  time.textContent = logStamp(performance.now() - bootAt);
  const text = el("span");
  text.textContent = words;
  item.append(time, text);
  for (const line of logList.children) line.removeAttribute("data-current");
  item.dataset.current = "true";
  logList.append(item);
  logList.scrollTop = logList.scrollHeight;
  i10.set(String(logList.children.length), "lines");
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

/** The four toolbar figures and the two the rail carries, from the game on screen. */
function paintIdentity(): void {
  t1.set(String(game.seed));
  t2.set(String(game.goal));
  t3.set(String(game.pieces));
  t4.set(String(game.beamWidth));
  e1.set(String(game.beamWidth), `of ${BOUNDS.widthCap} states per Layer`);
  c2.set(String(game.goal));
  presetName.textContent = `seed ${game.seed} · goal ${game.goal} · ${game.pieces} pieces · height ${game.heightCap}`;
}

/** The toolbar's outcome slot: a word, the Lines against the Goal, and the clock. */
function paintOutcome(): void {
  const goal = shown?.input.goal ?? game.goal;
  if (outcome === "solving") {
    outcomeLine.dataset.outcome = "solving";
    outcomeLine.textContent = `solving · seed ${game.seed} · goal ${goal} Lines`;
    app.dataset.solving = "true";
    return;
  }
  const lines = shown?.outcome.lines ?? 0;
  const won = shown?.outcome.won ?? false;
  outcomeLine.dataset.outcome = won ? "solved" : "lost";
  outcomeLine.textContent = `${won ? "solved" : "lost"} · ${lines} of ${goal} Lines cleared · ${elapsedMs.toFixed(1)} ms`;
  app.dataset.solving = "false";
  const t5 = FI.t5;
  t5?.set(`${lines} of ${goal}`);
  FI.t7?.set(won ? "solved" : "lost");
}

/** The status bar: Evaluations, Layers, Lines cleared, elapsed — in the glossary's words. */
function paintStatus(state?: PlaybackStatus): void {
  const goal = shown?.input.goal ?? game.goal;
  const evaluations = shown?.outcome.evaluations ?? 0;
  const layertotal = traced?.layerCount ?? 0;
  const played = state ? Math.min(state.played, layertotal) : playback ? playback.k : 0;
  const lines = state ? state.lines : 0;
  s1.set(evaluations.toLocaleString("en-US"), "");
  s2.set(`${Math.min(played, layertotal)} of ${layertotal}`, "");
  s3.set(`${lines} of ${goal}`);
  s4.set(elapsedMs.toFixed(1));
  i9.set(`${layertotal} of ${layertotal} Layers`);
}

/** Where a Move is in words, so no figure on the page is unlabelled. */
function describePosition(state: PlaybackStatus): string {
  const total = state.total;
  const k = Math.min(state.played, total);
  const pieceIndex = shown && k < total ? (shown.moves[k] >>> 16) & 0xff : null;
  const piece = pieceIndex === null ? "" : ` · next ${letterOf(pieceIndex)}`;
  const goal = shown ? ` of ${shown.input.goal}` : "";
  const pace = `${state.speed.toFixed(2)} Moves/s · ${(msPerMove(state.speed) / 1000).toFixed(2)} s per Move`;
  return `Move ${k} of ${total}${piece} · ${state.lines}${goal} Lines cleared · ${pace} · ${state.playing ? "playing" : "paused"}`;
}

// ── the readout, the table, the band ───────────────────────────────────────

/** The Layer on screen: the Moves played, or the failing Layer of a lost game. */
function layerOnScreen(): number {
  if (!traced || traced.layerCount === 0) return 0;
  const lost = shown !== null && shown.moves.length === 0;
  const k = lost ? traced.layerCount - 1 : playback ? playback.k : 0;
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
    glyph.innerHTML = pieceGlyph(letter, 6);
    glyph.dataset.tip = `${letter} — the piece ${at - k + 1} placement${at - k === 0 ? "" : "s"} from here, drawn from the 7-bag.`;
    nextSeq.append(glyph);
  }
  if (letters.length === 0) {
    const none = el("span", "chip");
    none.textContent = "the Sequence is spent";
    nextSeq.append(none);
  }
  nextWords.textContent = `${k} of ${sequence.length} pieces drawn`;
  c12.num.textContent = `${Math.min(4, Math.max(0, sequence.length - k))} of ${sequence.length}`;
  c12.unit.textContent = "pieces";
}

/** This Layer's counts: the trace's own records, never a recount. */
function paintLayerCounts(layer: number): void {
  const records = traced ? layerSlice(traced, layer) : [];
  if (records.length === 0) {
    c13.set("not measured");
    c15.set("not measured");
    c16.set("not measured");
    c17.set("not measured");
    return;
  }
  const best = records[0].score;
  const worst = records[records.length - 1].score;
  const atBest = records.filter((record) => record.score === best).length;
  c13.set(String(traced?.perLayerCounts[layer] ?? 0));
  c15.set(String(best));
  c16.set(String(atBest));
  c17.set(String(best - worst));
  c5.set(String(best));
  i5.set(`${best} in Layer ${layer}`, "score");
}

/** Score across Layers: the best score kept at each Layer, and where the playhead is. */
function paintSpark(layer: number): void {
  const scores = traced
    ? traced.perLayerCounts.map((_, at) => traced!.records[traced!.layerStart[at]]?.score ?? 0)
    : [];
  const line = sparkline(scores, 200, 54);
  const dot = line.points[layer];
  sparkHostSvg.innerHTML =
    line.points.length === 0
      ? `<p class="ro-empty">one point per Layer, left to right — the picture arrives with the first Solve</p>`
      : `<svg viewBox="0 -2 200 58" preserveAspectRatio="none" role="img" aria-label="Best score kept at each Layer">
          <path class="line" d="${line.path}"/>
          ${dot ? `<circle class="dot" cx="${dot.x}" cy="${dot.y}" r="2.2"/>` : ""}
        </svg>`;
  c18.set(
    scores.length === 0 ? "not measured" : `${Math.max(...scores)} best · ${Math.min(...scores)} worst`,
    scores.length === 0 ? "" : "score",
  );
}

/** Per-layer table: one row per Layer, every column the trace's own. */
function paintTable(currentLayer: number): void {
  if (!traced || !shown) {
    layerRowsBody.replaceChildren();
    const row = el("tr");
    const cell = el("td", "empty");
    cell.colSpan = 5;
    cell.textContent = "no Layer yet — every column of this table is the trace's, so it fills when the first Solve lands";
    row.append(cell);
    layerRowsBody.append(row);
    return;
  }
  const rows = layerRows(traced, shown.sequence);
  const built = rows.map((entry) => {
    const row = el("tr");
    if (entry.layer === currentLayer) row.dataset.current = "true";
    row.dataset.tip = `Layer ${entry.layer} · ${entry.piece} · best score ${entry.best} · ${entry.lines} Lines cleared · ${entry.kept} states kept`;
    const layer = el("td");
    layer.textContent = String(entry.layer).padStart(2, "0");
    const piece = el("td", "piece");
    piece.innerHTML = pieceGlyph(entry.piece, 7);
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
  sizeInspector();
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
}

/**
 * The inspector's vertical arithmetic (notes/14 §1.4): the band takes
 * `min(Layers × 22, 45 % of the inspector)`, the table gets `1fr`, the log is 96.
 * 45 % rather than 55 % so the per-layer table still shows the game at 1440×900.
 */
function sizeInspector(): void {
  const inner = inspector.clientHeight;
  if (inner <= 0) return;
  const layers = traced?.layerCount ?? 0;
  // The *canvas* gets the notes' budget, not the panel: one row per Layer at 22 px,
  // capped at 45 % of the inspector so the per-layer table and the log keep their
  // room at 1440×900 (notes/14 §1.4 — 55 % leaves the table 11 rows).
  const bandH = Math.min(Math.max(120, layers * 22 || 120), Math.round(inner * 0.45));
  bandCanvas.style.height = `${bandH}px`;
  bandCanvas.style.flex = "0 0 auto";
  bandPanel.style.height = "";
}

// ── painting the whole page from one state ─────────────────────────────────
function paintAll(state?: PlaybackStatus): void {
  const layer = layerOnScreen();
  const k = state ? Math.min(state.played, state.total) : playback ? playback.k : 0;
  paintIdentity();
  paintOutcome();
  paintStatus(state);
  paintColumns(k);
  paintPlacement(k);
  paintNext(k);
  paintLayerCounts(layer);
  paintSpark(layer);
  paintTable(layer);
  c1.set(`${layer} of ${traced?.layerCount ?? 0}`, "");
  const step = playback && k > 0 ? playback.timeline.steps[k - 1] : null;
  const nextLetter = step ? letterOf(step.piece) : shown ? letterOf((shown.moves[0] >>> 16) & 0xff) : "?";
  pieceNow.innerHTML = `${svgIcon("brand-mark")}<span class="word">${step ? "placed" : "next"}</span>${pieceGlyph(nextLetter, 5)}<span class="word">${nextLetter}</span>`;
  pieceNow.dataset.tip = `The Piece this Move placed, drawn from the Sequence the Generator fixed before the search started.`;
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
      if (mode === "tick") cell.dataset.tick = "true";
      strip.append(cell);
    }
    strip.dataset.mode = mode;
    strip.setAttribute("aria-valuemax", String(total));
    m1.set(String(total), "cells");
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
  const words = `No Moves to replay: the search stopped at ${message.outcome.lines} Lines cleared.`;
  say(progressLine, words);
  for (const button of [playButton, prevButton, nextButton, restartButton]) button.disabled = true;
  setPlayButton("play", false);
  newGameStrip(words);
}

/** The strip, when there is nothing to play: one labelled cell instead of a hole. */
function newGameStrip(words: string): void {
  strip.replaceChildren();
  const cell = el("i");
  cell.dataset.empty = "true";
  cell.textContent = "no Moves — the search ended before the first Move";
  cell.dataset.tip = words;
  strip.dataset.mode = "none";
  strip.append(cell);
  strip.setAttribute("aria-valuemax", "0");
  strip.setAttribute("aria-valuenow", "0");
  strip.setAttribute("aria-valuetext", "No Moves to replay");
  caret.style.left = "0px";
  painted = { k: 0, total: 0 };
  clearedMoveFlag = [];
  m1.set("0", "cells");
  m2.set("0", "of 0 Moves");
}

function setPlayButton(word: "play" | "pause" | "replay", pressed: boolean): void {
  playWord.textContent = word;
  playIcon.innerHTML = svgIcon(word === "pause" ? "pause" : "play", 10);
  playButton.setAttribute("aria-pressed", String(pressed));
  playButton.setAttribute("aria-label", word === "replay" ? "Replay from the start" : word === "pause" ? "Pause" : "Play");
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
  say(
    statusLine,
    `${describeGame(input)} · beam ${input.beamWidth}`,
  );

  if (playback) playback.stop();
  playback = null;
  shownBoard = fieldFrom(message.board);
  view.resize();
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
      say(progressLine, describePosition(state));
      paintStrip(Math.min(state.played, state.total), state.total);
      setPlayButton(state.atEnd ? "replay" : state.playing ? "pause" : "play", state.playing);
      paintStatus(state);
      const layer = layerOnScreen();
      paintColumns(Math.min(state.played, state.total));
      paintPlacement(Math.min(state.played, state.total));
      paintNext(Math.min(state.played, state.total));
      paintLayerCounts(layer);
      paintSpark(layer);
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
 * put 500 words, and a drawer that reflowed the grid would move the Board — which is
 * the one thing §2 says must not happen.
 */
function setDrawer(open: boolean): void {
  drawer.dataset.open = String(open);
  drawer.setAttribute("aria-hidden", String(!open));
  scrim.dataset.open = String(open);
  if (!open) setSheet(false);
  else drawer.focus();
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
treeButton.addEventListener("click", () => {
  const open = treeButton.getAttribute("aria-pressed") !== "true";
  treeButton.setAttribute("aria-pressed", String(open));
  treePanel.hidden = !open;
  sizeInspector();
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
  sizeInspector();
  view.resize();
  bandView.resize();
  bandView.draw(bandModel);
  if (playback) {
    playback.draw();
    painted = { k: -1, total: painted.total };
    paintStrip(currentK(), painted.total);
  } else {
    view.draw(shownBoard);
  }
});

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
sizeInspector();
view.resize();
view.draw(shownBoard);
paintAll();
log("page loaded");
spawn();
