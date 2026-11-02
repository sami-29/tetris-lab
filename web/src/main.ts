// The page: boot the worker, ask for the game the URL names, and make the Solve
// something a visitor can watch properly.
//
// Ticket 01 stopped at "it plays back". Ticket 10's half is a continuous speed
// control, a move strip that is a seek bar, pause / resume / restart from the
// keyboard and from a control, and a keyboard map a visitor can find. It owns no
// animation of its own — `ui/playback.ts` draws every frame from `(k, into)`,
// `ui/clock.ts` owns the playhead and `ui/timeline.ts` owns the boards — so this
// file is the wiring: one listener per input, one repaint per change.
//
// Ticket 13's half is the game itself: the Playground (`ui/playground.ts`) commits
// a clamped `GameInput`, and this file does the three things a commit means —
//
//   1. **clamp** (already done by the Playground, and again here, because the URL is
//      also an input);
//   2. **write the URL** with `history.replaceState`, *before* asking for the solve,
//      so a link copied during a 0.2 s solve still names the game the visitor asked
//      for and a failed solve cannot leave the address bar disagreeing with the
//      controls;
//   3. **post the solve**, one in flight and one queued — the worker cannot be
//      preempted (its handler runs `gen_game` + `solve` synchronously), so the queue
//      is a single latest-value slot rather than a backlog.
//
// `replaceState`, never `pushState`: a slider drag is 90–180 `input` events, and a
// pushed entry per commit would make Back walk the beam slider one tick at a time
// instead of leaving the page. With no pushes there is nothing to pop within the
// document, so there is no `popstate` listener either.
//
// There is still no visual system here. Ticket 14 replaces this shell; what it must
// find intact is the behaviour and `PALETTE` (the canvas's whole colour system).

import engineWasmUrl from "../pkg/engine.wasm?url";

import { DEFAULT_BEAM, WIDTH_CAP } from "./config/width.ts";
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
import { fieldFrom, emptyField, type Field } from "./ui/field.ts";
import { BoardView } from "./ui/board.ts";
import { Playback, type PlaybackStatus } from "./ui/playback.ts";
import { Playground } from "./ui/playground.ts";
import { letterOf } from "./ui/pieces.ts";
import { MPS_DEFAULT, MPS_MAX, MPS_MIN, mpsToSlider, msPerMove, sliderToMps } from "./ui/phases.ts";
import { KEY_MAP, SPEED_STOP, resolveKey, type Action } from "./ui/keys.ts";
import { moveToX, stripMode, xToMove } from "./ui/seek.ts";

/**
 * §4's handshake number, echoed in the status line. The loader checks it.
 */
const ABI = 2;

/**
 * The two numbers the page hangs on, read at build time from the committed width
 * curve (`data/measurements/width-curve.json`, measured by `bench width-curve` for
 * §9.5) through `./config/width.ts` — the one module allowed to name them. The URL's
 * default `beam`, the Playground's slider maximum and the tooltip are all built from
 * this pair, so none of them can drift from the published table.
 */
const BOUNDS: Bounds = { defaultBeam: DEFAULT_BEAM, widthCap: WIDTH_CAP };

/** Every element the page needs, or a loud failure at boot. */
function must<T extends Element>(selector: string): T {
  const element = document.querySelector<T>(selector);
  if (!element) throw new Error(`index.html is missing ${selector}`);
  return element;
}

const canvas = must<HTMLCanvasElement>("#board");
const statusLine = must<HTMLElement>("#status");
const outcomeLine = must<HTMLElement>("#outcome");
const progressLine = must<HTMLElement>("#progress");
const playButton = must<HTMLButtonElement>("#play");
const restartButton = must<HTMLButtonElement>("#restart");
const speedSlider = must<HTMLInputElement>("#speed");
const speedReadout = must<HTMLOutputElement>("#speed-readout");
const strip = must<HTMLElement>("#strip");
const caret = must<HTMLElement>("#caret");
const sheet = must<HTMLElement>("#sheet");
const sheetList = must<HTMLElement>("#sheet-list");
const keysButton = must<HTMLButtonElement>("#keys-button");
const playgroundRoot = must<HTMLElement>("#playground");

const view = new BoardView(canvas);
const lines = { status: statusLine, outcome: outcomeLine, progress: progressLine };

let worker: Worker | null = null;
let playback: Playback | null = null;
/** The game last shown, for the readouts that outlive a frame. */
let shown: GameMessage | null = null;
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

/**
 * The game the page is showing, and the game it has asked for. They differ only
 * while a solve is in flight, and the URL always names the one that was *asked for*
 * (the input), never the one that came back (the output).
 */
let game: GameInput = defaultGame(BOUNDS);
/** The Board on screen, so a resize repaints what is there rather than blanking it. */
let shownBoard: Field = emptyField();
/** Set by the worker's `ready`, cleared by a respawn: a request before it would fail. */
let booted = false;
/** True while a solve is in the worker; the worker cannot be preempted. */
let inFlight = false;
/** The one pending game: a drag's intermediate values overwrite each other here. */
let queued: GameInput | null = null;

function say(element: HTMLElement, text: string, tone: "plain" | "error" = "plain"): void {
  element.textContent = text;
  element.dataset.tone = tone;
}

function spawn(): void {
  if (worker) worker.terminate();
  booted = false;
  inFlight = false;
  worker = new Worker(new URL("./workers/solve.worker.ts", import.meta.url), {
    type: "module",
  });
  worker.onmessage = (event: MessageEvent<FromWorker>) => receive(event.data);
  worker.onerror = (event: ErrorEvent) => {
    say(lines.status, `worker error: ${event.message || "unknown"}`, "error");
  };
  const boot: ToWorker = { type: "boot", wasmUrl: engineWasmUrl };
  worker.postMessage(boot);
}

/**
 * Ask for one game. One in flight, one pending, latest wins: the worker runs
 * `gen_game` + `solve` synchronously inside its message handler, so a second
 * request cannot interrupt the first — a queue longer than one would only grow.
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
  const message: ToWorker = { type: "solve", requestId: pending, ...input };
  say(lines.status, `${describeGame(input)} — solving…`);
  say(lines.outcome, "");
  worker.postMessage(message);
}

/** The pending game, if any, once the worker is free. */
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
      say(
        lines.status,
        `wasm ready · abi ${message.abiVersion} · scratch at 0x${message.scratchOffset.toString(16)}`,
      );
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
      say(lines.status, `${fatal ? "fatal" : "error"}: ${message.message}`, "error");
      say(lines.outcome, "", "error");
      // A trap costs the instance, so a fresh worker replaces it. A mismatch does
      // not: retrying a stale artifact forever would be worse than saying so.
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

/* ---- the game: clamp, write the URL, ask for the solve ----------------- */

/**
 * The one commit path, shared by the Playground, the presets and the load path.
 *
 * Order matters (notes 13 §1.5): clamp → write the URL → post the solve. The URL is
 * the *input* and the Board is the *output*, so a link copied mid-solve still names
 * the game the visitor asked for, and no code path has to roll the URL back.
 */
function applyGame(candidate: GameInput): void {
  const next = clampGame(candidate, BOUNDS);
  const reason = reSolveReason(next, game);
  playgroundRoot.dataset.reason = reason;
  if (reason === "none") return; // the same game, or nothing to change
  game = next;
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
    // Only reachable when the document has no usable URL (a `srcdoc` frame, say).
    // The solve still goes ahead: the visitor asked for a game, and the address bar
    // failing to follow it is not a reason to withhold it.
    console.warn(`[tetris-lab] could not write the URL: ${String(error)}`);
  }
}

/** Where a Move is in words, so no figure on the page is unlabelled. */
function describePosition(state: PlaybackStatus): string {
  const total = state.total;
  const k = Math.min(state.played, total);
  const pieceIndex = shown && k < total ? (shown.moves[k] >>> 16) & 0xff : null;
  const piece = pieceIndex === null ? "" : ` · next ${letterOf(pieceIndex)}`;
  const goal = shown ? ` of ${shown.input.goal}` : "";
  const pace = `${state.speed.toFixed(2)} moves/s · ${(msPerMove(state.speed) / 1000).toFixed(2)} s per move`;
  return `Move ${k} of ${total}${piece} · ${state.lines}${goal} lines cleared · ${pace} · ${state.playing ? "playing" : "paused"}`;
}

/**
 * The solve's outcome in words (§6.2): the Goal it was given and the Lines it
 * cleared, then the figures behind them. A loss reads as an outcome — "Lost: …
 * was not reached" — and is never given the error tone, so the page cannot present
 * a game it could not win as a fault.
 */
function describeOutcome(message: GameMessage): string {
  const { input, outcome } = message;
  const headline = outcome.won
    ? `Won: the Goal of ${input.goal} lines was reached, ${outcome.lines} Lines cleared.`
    : `Lost: the Goal of ${input.goal} lines was not reached, ${outcome.lines} Lines cleared.`;
  const figures =
    `${outcome.evaluations} evaluations · ${outcome.moveCount} Moves` +
    ` · solve ${message.elapsedMs.toFixed(1)} ms`;
  return `${headline} · ${figures}`;
}

function paintStrip(k: number, total: number): void {
  if (painted.total !== total) {
    strip.replaceChildren();
    const mode = stripMode(total);
    for (let index = 0; index < total; index += 1) {
      const cell = document.createElement("i");
      const word = shown?.moves[index];
      const clears = word === undefined ? false : Boolean(clearedMoveFlag[index]);
      if (clears) cell.dataset.clears = "true";
      cell.title =
        word === undefined
          ? `Move ${index + 1}`
          : `Move ${index + 1} · ${letterOf((word >>> 16) & 0xff)}${clears ? " · clears rows" : ""}`;
      if (mode === "tick") cell.dataset.tick = "true";
      strip.append(cell);
    }
    strip.dataset.mode = mode;
    strip.setAttribute("aria-valuemax", String(total));
    painted = { k: -1, total };
  }
  strip.setAttribute("aria-valuenow", String(k));
  strip.setAttribute("aria-valuetext", `Move ${k} of ${total}`);
  // Repaint one index, not the whole strip: at 60 Hz a 200-Move strip would
  // otherwise be 12,000 DOM writes a second.
  if (painted.k === k) return;
  const cells = strip.children;
  for (let index = 0; index < cells.length; index += 1) {
    const cell = cells[index];
    const played = index < k;
    if (cell.getAttribute("data-played") !== String(played)) {
      cell.setAttribute("data-played", String(played));
    }
    const current = index === k - 1;
    if (cell.getAttribute("data-current") !== String(current)) {
      cell.setAttribute("data-current", String(current));
    }
  }
  const width = strip.clientWidth;
  caret.style.left = `${moveToX(k, width, total) - 1}px`;
  painted.k = k;
}

/** Which Moves clear rows, computed once per game from the replay. */
let clearedMoveFlag: boolean[] = [];

/**
 * A game the search could not win is still an outcome (ticket bullet 8).
 *
 * The loss path returns `status 0`, `won false`, the Lines the search reached and
 * **no Move list at all**, so there is nothing to animate and the strip has nothing
 * to seek. The page therefore says what happened in words, draws the Board the game
 * was given, and disables the transport rather than leaving a play button that plays
 * nothing.
 */
function showLoss(message: GameMessage): void {
  const words = `No Moves to replay: the search stopped at ${message.outcome.lines} Lines cleared.`;
  say(lines.progress, words);
  playButton.disabled = true;
  restartButton.disabled = true;
  playButton.textContent = "play";
  playButton.setAttribute("aria-pressed", "false");
  playButton.setAttribute("aria-label", "Play / pause");
  newGameStrip(words);
}

/** The strip, when there is nothing to play: one labelled cell instead of a hole. */
function newGameStrip(words: string): void {
  strip.replaceChildren();
  const cell = document.createElement("i");
  cell.dataset.empty = "true";
  cell.textContent = "no Moves";
  cell.title = words;
  strip.dataset.mode = "none";
  strip.append(cell);
  strip.setAttribute("aria-valuemax", "0");
  strip.setAttribute("aria-valuenow", "0");
  strip.setAttribute("aria-valuetext", "No Moves to replay");
  caret.style.left = "0px";
  painted = { k: 0, total: 0 };
  clearedMoveFlag = [];
}

function show(message: GameMessage): void {
  const { input } = message;
  shown = message;

  say(
    lines.status,
    `${describeGame(input)} · wasm abi ${ABI} · ${message.sequence.length} pieces in the Sequence`,
  );
  say(lines.outcome, describeOutcome(message));
  outcomeLine.dataset.outcome = message.outcome.won ? "won" : "lost";

  if (playback) playback.stop();
  playback = null;
  shownBoard = fieldFrom(message.board);
  view.resize();
  view.draw(shownBoard);

  if (message.moves.length === 0) {
    showLoss(message);
    return;
  }

  playButton.disabled = false;
  restartButton.disabled = false;
  playback = new Playback(view, message, {
    speed,
    onStatus(state) {
      say(lines.progress, describePosition(state));
      paintStrip(Math.min(state.played, state.total), state.total);
      playButton.textContent = state.atEnd ? "replay" : state.playing ? "pause" : "play";
      playButton.setAttribute("aria-pressed", String(state.playing));
      playButton.setAttribute("aria-label", state.atEnd ? "Replay from the start" : state.playing ? "Pause" : "Play");
    },
  });

  clearedMoveFlag = playback.timeline.steps.map((step) => step.cleared.length > 0);
  painted = { k: -1, total: -1 };
  // Announce Move 0 now rather than on the first frame: otherwise the progress line
  // keeps the previous game's words (a `No Moves to replay` from a loss, say) for as
  // long as it takes this one to start playing, and an empty or stale interior is a
  // defect (§2.1.1).
  playback.seek(0);

  // Show the starting position before anything moves: the solve is about a real
  // board, and a visitor should see it.
  if (startTimer !== 0) clearTimeout(startTimer);
  startTimer = globalThis.setTimeout(() => {
    startTimer = 0;
    playback?.play();
  }, 400);
}

/* ---- controls ---------------------------------------------------------- */

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
  const label = `${speed.toFixed(2)} moves/s · ${(msPerMove(speed) / 1000).toFixed(2)} s per move`;
  speedReadout.textContent = label;
  speedSlider.setAttribute("aria-valuetext", label);
}

function run(action: Action): void {
  // The map's playback keys need a Move list; the sheet's keys do not, so a lost
  // game still has a readable keyboard map and a working Escape.
  switch (action.type) {
    case "keys":
      setSheet(sheet.dataset.open !== "true");
      return;
    case "close":
      setSheet(false);
      return;
    case "speed": {
      // One key press is one stop on the log scale, so the keys land on the same
      // lattice the slider does.
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
 * Show or hide the keyboard map.
 *
 * Focus moves to the sheet (so Escape and the map's own keys are the page's, not a
 * focused button's) and back to the strip on close (so Space is play/pause again
 * rather than "click the button that was last pressed").
 */
function setSheet(open: boolean): void {
  sheet.dataset.open = String(open);
  if (open) sheet.focus();
  else strip.focus();
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
  // Playing from a scrubbed position continues from that Move's own board: the
  // clock resumes at `k`, and step `k + 1` starts from `reconstruct(k)`.
  if (resumeAfterDrag) playback.play();
  resumeAfterDrag = false;
};

strip.addEventListener("pointerup", endDrag);
strip.addEventListener("pointercancel", endDrag);

/* ---- the transport ---------------------------------------------------- */

playButton.addEventListener("click", () => {
  playback?.toggle();
});

restartButton.addEventListener("click", () => {
  playback?.restart();
});

speedSlider.addEventListener("input", () => {
  setSpeed(sliderToMps(Number(speedSlider.value)));
});

/* ---- the keyboard ----------------------------------------------------- */

globalThis.addEventListener("keydown", (event) => {
  const action = resolveKey(event);
  if (!action) return;
  // Every key in the map is one the browser would otherwise use (scrolling,
  // find-as-you-type, the focused control's own keys), so take it deliberately.
  event.preventDefault();
  run(action);
});

// A hidden tab's animation frames are unpredictable, so pause instead of trusting
// them: the clock's stall guard covers the rest (`STALL_MS`).
document.addEventListener("visibilitychange", () => {
  if (document.hidden && playback?.playing) playback.pause();
});

/* ---- the keyboard map, in words -------------------------------------- */

for (const { keys: chord, does } of KEY_MAP) {
  const term = document.createElement("dt");
  for (const key of chord.split(" ")) {
    const chip = document.createElement("span");
    chip.className = "kbd";
    chip.textContent = key;
    term.append(" ", chip);
  }
  const detail = document.createElement("dd");
  detail.textContent = does;
  sheetList.append(term, detail);
}
sheet.addEventListener("click", (event) => {
  if (event.target === sheet) setSheet(false);
});
keysButton.addEventListener("click", () => setSheet(sheet.dataset.open !== "true"));

/* ---- first paint ------------------------------------------------------ */

globalThis.addEventListener("resize", () => {
  view.resize();
  if (playback) {
    playback.draw();
    painted = { k: -1, total: painted.total };
    paintStrip(currentK(), painted.total);
  } else {
    // A lost game has no playhead, but it does have a Board: repaint that rather
    // than blanking the stage.
    view.draw(shownBoard);
  }
});

/* ---- the Playground and the load path --------------------------------- */

/**
 * The Playground commits a clamped game; this file owns what a commit means (the
 * URL and the solve). It is mounted before wasm exists, so the controls show the
 * clamped game even if the artifact never boots.
 */
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

setSpeed(speed);
sheet.dataset.open = "false";
view.resize();
view.draw(shownBoard);
spawn();
