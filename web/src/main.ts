// The page: boot the worker, ask for the default game, draw the starting Board,
// then play the engine's Move list back piece by piece.
//
// Ticket 01 stops here on purpose. There are no controls — the seed, the goal, the
// piece count, the height cap and the Beam width are the defaults of §2.2 — and no
// visual system; both arrive in later tickets (13 and 14). What this file has to be
// is *correct*: every figure it prints came out of wasm, and the board it draws is
// the board the engine solved.

import engineWasmUrl from "../pkg/engine.wasm?url";

import { DEFAULT_BEAM } from "./config/width.ts";
import type { FromWorker, GameMessage, GameInput, ToWorker } from "./core/protocol.ts";
import { fieldFrom } from "./ui/field.ts";
import { BoardView } from "./ui/board.ts";
import { Playback } from "./ui/playback.ts";
import { letterOf } from "./ui/pieces.ts";

/**
 * §2.2's defaults. `DEFAULT_BEAM` is not written here: it is read at build time
 * from the committed width-curve artifact (`data/measurements/width-curve.json`,
 * measured by `bench width-curve` for §9.5), through `./config/width.ts` — the
 * one module allowed to name it. The toolbar's width field opens at it, the
 * slider runs to `WIDTH_CAP`, and neither number can drift from the published
 * table because all three read the same import.
 */
/** §4's handshake number, echoed in the status line. The loader checks it. */
const ABI = 2;

const DEFAULTS: GameInput = {
  seed: 0,
  goal: 8,
  pieces: 40,
  heightCap: 7,
  beamWidth: DEFAULT_BEAM,
};

const canvas = document.querySelector<HTMLCanvasElement>("#board");
const statusLine = document.querySelector<HTMLElement>("#status");
const outcomeLine = document.querySelector<HTMLElement>("#outcome");
const progressLine = document.querySelector<HTMLElement>("#progress");

if (!canvas || !statusLine || !outcomeLine || !progressLine) {
  throw new Error("index.html is missing #board, #status, #outcome or #progress");
}

const view = new BoardView(canvas);
const status = { line: statusLine, outcome: outcomeLine, progress: progressLine };

let worker: Worker | null = null;
let playback: Playback | null = null;
let startTimer = 0;
let nextRequestId = 1;
let pending = 0;

function say(element: HTMLElement, text: string, tone: "plain" | "error" = "plain"): void {
  element.textContent = text;
  element.dataset.tone = tone;
}

function describe(input: GameInput): string {
  return `seed ${input.seed} · goal ${input.goal} lines · ${input.pieces} pieces · cap ${input.heightCap} · beam ${input.beamWidth}`;
}

function spawn(): void {
  if (worker) worker.terminate();
  worker = new Worker(new URL("./workers/solve.worker.ts", import.meta.url), {
    type: "module",
  });
  worker.onmessage = (event: MessageEvent<FromWorker>) => receive(event.data);
  worker.onerror = (event: ErrorEvent) => {
    say(status.line, `worker error: ${event.message || "unknown"}`, "error");
  };
  const boot: ToWorker = { type: "boot", wasmUrl: engineWasmUrl };
  worker.postMessage(boot);
}

function requestGame(input: GameInput): void {
  if (!worker) return;
  pending = nextRequestId;
  nextRequestId += 1;
  const message: ToWorker = { type: "solve", requestId: pending, ...input };
  say(status.line, `${describe(input)} — solving…`);
  say(status.outcome, "");
  worker.postMessage(message);
}

function receive(message: FromWorker): void {
  switch (message.type) {
    case "ready": {
      say(status.line, `wasm ready · abi ${message.abiVersion} · scratch at 0x${message.scratchOffset.toString(16)}`);
      requestGame(DEFAULTS);
      return;
    }
    case "game": {
      if (message.requestId !== pending) return; // a stale answer
      show(message);
      return;
    }
    case "error": {
      if (message.requestId !== 0 && message.requestId !== pending) return;
      const fatal = message.code === "abi-mismatch";
      say(status.line, `${fatal ? "fatal" : "error"}: ${message.message}`, "error");
      say(status.outcome, "", "error");
      // A trap costs the instance, so a fresh worker replaces it. A mismatch does
      // not: retrying a stale artifact forever would be worse than saying so.
      if (!fatal) {
        console.warn(`[tetris-lab] ${message.code}: ${message.message}`);
        if (message.code === "wasm-trap") spawn();
      }
      return;
    }
  }
}

function show(game: GameMessage): void {
  const { input, outcome } = game;

  say(status.line, `${describe(input)} · wasm abi ${ABI} · ${game.sequence.length} pieces in the Sequence`);

  const verdict = outcome.won ? "won" : "lost";
  say(
    status.outcome,
    `${verdict} · ${outcome.lines} of ${input.goal} lines cleared · ${outcome.evaluations} evaluations · ${outcome.moveCount} moves · ${game.elapsedMs.toFixed(1)} ms`,
  );

  if (playback) playback.stop();
  say(status.progress, `move 1 of ${outcome.moveCount} · 0 of ${outcome.lines} lines replayed`);
  view.resize();
  view.draw(fieldFrom(game.board));

  playback = new Playback(view, game, {
    onStatus(state) {
      const next = game.moves[state.played];
      const piece = next === undefined ? null : letterOf((next >>> 16) & 0xff);
      const move = state.played + 1 > state.total ? state.total : state.played + 1;
      say(
        status.progress,
        `move ${move} of ${state.total}${piece ? ` · ${piece}` : ""} · ${state.lines} of ${outcome.lines} lines replayed`,
      );
    },
  });

  // Show the starting position before anything moves: the solve is about a real
  // board, and a visitor should see it.
  if (startTimer !== 0) clearTimeout(startTimer);
  startTimer = globalThis.setTimeout(() => {
    startTimer = 0;
    playback?.start();
  }, 400);
}

globalThis.addEventListener("resize", () => {
  view.resize();
  if (!playback) view.draw(fieldFrom(new Uint32Array(10)));
});

view.resize();
view.draw(fieldFrom(new Uint32Array(10)));
spawn();
