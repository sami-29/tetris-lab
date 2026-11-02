// Playback: what the page draws, frame by frame, from `(k, into)`.
//
// The Move list, the Lines and the outcome are the engine's; the *picture* is this
// file's job, and it is a pure function of the timeline and the clock in
// `clock.ts` — no animation object, no tweening library, no CSS transition that the
// canvas cannot see. Every frame it asks "which Move, how far into it" and paints
// that. Which is why pausing keeps the interpolated pose, a scrub simply draws
// `reconstruct(k)`, and a speed change retimes the same animation instead of
// restarting it.
//
// Two motions carry the ticket:
//
//   * **a drop lands.** The piece falls from §5.3's spawn row to the engine's
//     resting shift with an accelerating fall (`t²`), then touches down with a
//     contact edge for a beat. It is never stamped at the bottom.
//   * **a clear collapses.** The cleared rows contract away from their top edge
//     while every surviving run falls into the gap — each run by its own distance,
//     because a clear need not be contiguous. A translucent flash draws none of
//     that, which is why `web/test/playback-animation.test.mjs` asserts the bands
//     contract and the runs move.
//
// The scheduler and the render target are injected so `node --test` can own the
// clock and read the frames: that is what makes "a drop visibly lands" a checked
// claim rather than a screenshot.

import type { GameMessage } from "../core/protocol.ts";
import { H, W, emptyField, type Field } from "./field.ts";
import { unpackMove, buildTimeline, type Step, type Timeline } from "./timeline.ts";
import { FRAME_MS, MPS_DEFAULT, clampMps, msPerMove, phases } from "./phases.ts";
import {
  initialState,
  pause as pauseState,
  position,
  reanchor,
  restart as restartState,
  resume as resumeState,
  seek as seekState,
  setSpeed as setSpeedState,
  toggle as toggleState,
  type PlayState,
  type Position,
} from "./clock.ts";
import type { Overlay } from "./board.ts";

export interface PlaybackStatus {
  /** Moves played: the index of the step on screen, `0..total`. */
  played: number;
  total: number;
  /** Lines cleared by the replay up to here. */
  lines: number;
  /** The board row (0 = top) the piece in flight would land on, if any. */
  landingRow: number | null;
  playing: boolean;
  atEnd: boolean;
  /** Moves per second. */
  speed: number;
  /** How far into the current Move, `0..1`. */
  fraction: number;
}

/** What playback draws on. `BoardView` satisfies this structurally. */
export interface RenderTarget {
  draw(field: Field, overlay?: Overlay): void;
}

/** Time and frames, so a test can own them. */
export interface Scheduler {
  frame(callback: (now: number) => void): number;
  cancel(handle: number): void;
  now(): number;
}

export const browserScheduler: Scheduler = {
  frame: (callback) => requestAnimationFrame(callback),
  cancel: (handle) => cancelAnimationFrame(handle),
  now: () => performance.now(),
};

export interface PlaybackOptions {
  /** Moves per second: the continuous control's value. */
  speed?: number;
  scheduler?: Scheduler;
  /** A frame time to plan with when the display has not been measured yet. */
  frameMs?: number;
  onStatus?(status: PlaybackStatus): void;
  /** Reuse a timeline (the page builds it once per Solve); otherwise one is built. */
  timeline?: Timeline;
}

/** The `easeOutCubic` the prototype shipped, hand-written: nothing to import. */
function easeOutCubic(t: number): number {
  const p = 1 - Math.min(1, Math.max(0, t));
  return 1 - p * p * p;
}

export class Playback {
  readonly timeline: Timeline;

  private readonly view: RenderTarget;
  private readonly scheduler: Scheduler;
  private readonly onStatus: (status: PlaybackStatus) => void;

  private state: PlayState;
  private handle = 0;
  private frameMs: number;
  private lastFrame: number;
  private announced = -1;

  constructor(view: RenderTarget, game: GameMessage, options: PlaybackOptions = {}) {
    this.view = view;
    this.scheduler = options.scheduler ?? browserScheduler;
    this.onStatus = options.onStatus ?? (() => {});
    this.timeline = options.timeline ?? buildTimeline(game.board, [...game.moves]);
    this.state = initialState(clampMps(options.speed ?? MPS_DEFAULT), this.scheduler.now());
    this.frameMs = options.frameMs ?? FRAME_MS;
    this.lastFrame = this.state.t;
  }

  get playing(): boolean {
    return this.state.playing;
  }

  get speed(): number {
    return this.state.mps;
  }

  /** Which Move is on screen: `0` is the starting Board. */
  get k(): number {
    return this.state.k;
  }

  /** How far into the Move on screen, in milliseconds. */
  get into(): number {
    return this.state.into;
  }

  get atEnd(): boolean {
    return this.state.k >= this.timeline.steps.length;
  }

  /** Start from the beginning and play: the page's first paint, and its restart. */
  start(): void {
    this.restart();
  }

  play(): void {
    if (this.atEnd) {
      this.restart();
      return;
    }
    this.state = resumeState(this.timeline, this.state, this.scheduler.now());
    this.schedule();
    this.announce();
  }

  pause(): void {
    this.state = pauseState(this.timeline, this.state, this.scheduler.now());
    this.cancel();
    this.render(position(this.timeline, this.state, this.scheduler.now()));
    this.announce();
  }

  toggle(): void {
    this.state = toggleState(this.timeline, this.state, this.scheduler.now());
    if (this.state.playing) {
      this.schedule();
      this.announce();
    } else {
      this.cancel();
      this.render(position(this.timeline, this.state, this.scheduler.now()));
      this.announce();
    }
  }

  restart(): void {
    const now = this.scheduler.now();
    this.state = restartState(this.timeline, this.state, now);
    this.lastFrame = now;
    this.announced = -1;
    this.render(position(this.timeline, this.state, now));
    this.schedule();
    this.announce();
  }

  /** Stop the clock and leave the picture alone. */
  stop(): void {
    this.state = pauseState(this.timeline, this.state, this.scheduler.now());
    this.cancel();
  }

  /**
   * Put the playhead on Move `k` and pause there. The next draw is
   * `reconstruct(k)`, so the board on screen *is* the engine's board at that Move;
   * resuming plays Move `k + 1` onto it.
   */
  seek(k: number): void {
    const now = this.scheduler.now();
    this.state = seekState(this.timeline, this.state, k, now);
    this.lastFrame = now;
    this.cancel();
    this.render(position(this.timeline, this.state, now));
    this.announce();
  }

  /** Change speed without moving the piece: the fraction of the Move is preserved. */
  setSpeed(mps: number): void {
    const now = this.scheduler.now();
    this.state = setSpeedState(this.timeline, this.state, mps, now);
    if (this.state.playing) this.lastFrame = now;
    this.announce();
  }

  /** Draw the current position without touching the clock. */
  draw(): void {
    this.render(position(this.timeline, this.state, this.scheduler.now()));
  }

  private schedule(): void {
    if (this.handle !== 0) return;
    this.handle = this.scheduler.frame(this.tick);
  }

  private cancel(): void {
    if (this.handle !== 0) this.scheduler.cancel(this.handle);
    this.handle = 0;
  }

  private readonly tick = (now: number): void => {
    this.handle = 0;
    // Measure the display, once per frame, so `phases` knows whether a step is
    // long enough to draw. Smoothed, because one long frame is not a frame rate.
    const delta = now - this.lastFrame;
    if (delta > 0 && delta < 250) this.frameMs = this.frameMs * 0.8 + delta * 0.2;
    this.lastFrame = now;

    const at = position(this.timeline, this.state, now);
    if (at.k !== this.announced) this.announce(at);
    this.render(at);
    this.state = reanchor(this.timeline, this.state, now);

    if (at.atEnd) {
      // The Move list is spent: hold the finished board and let the page's play
      // button become a restart.
      this.state = { ...this.state, playing: true, k: at.k, into: 0, t: now };
      return;
    }
    this.handle = this.scheduler.frame(this.tick);
  };

  /** Paint the position: a drop in flight, a landing, a collapse, or the board. */
  private render(at: Position): void {
    const total = this.timeline.steps.length;
    if (at.k >= total) {
      this.view.draw(total === 0 ? emptyField() : this.timeline.steps[total - 1].postClear);
      return;
    }

    const step = this.timeline.steps[at.k];
    const plan = phases(msPerMove(this.state.mps), step.cleared.length, this.frameMs);

    if (plan.mode === "skip" || plan.dropMs <= 0) {
      // Too fast to animate: the finished position is the honest picture.
      this.view.draw(step.postClear);
      return;
    }

    if (at.into < plan.dropMs) {
      const t = at.into / plan.dropMs;
      const offset = step.dropRows * t * t;
      // The piece is drawn as an overlay, never stamped, until it lands.
      this.view.draw(step.preBoard, {
        piece: step.spawnCells.map((cell) => ({ col: cell.col, row: cell.row + offset })),
        ghost: step.restCells,
      });
      return;
    }

    const since = at.into - plan.dropMs;

    // A landing, not a stamp: the piece touches down with a contact edge for a
    // beat, whether or not it completed rows. The board is still preBoard here, so
    // the piece is drawn as an overlay in both frames of the moment it arrives.
    if (since < Math.min(90, plan.dropMs / 3)) {
      this.view.draw(step.preBoard, { piece: step.restCells, impact: true });
      return;
    }

    if (step.cleared.length === 0) {
      this.view.draw(step.preClear);
      return;
    }

    if (since >= plan.clearMs) {
      this.view.draw(step.postClear);
      return;
    }

    // The clear: the bands contract first, so the gap opens before anything falls
    // into it, and then each surviving run falls its own distance.
    const contract = plan.contractMs > 0 ? Math.min(1, since / plan.contractMs) : 1;
    const squeeze = 1 - 0.88 * easeOutCubic(contract);
    const fall = plan.fallMs > 0 ? Math.max(0, Math.min(1, (since - plan.contractMs) / plan.fallMs)) : 1;
    const drop = fall * fall;
    this.view.draw(emptyField(), {
      cells: this.survivingCells(step, drop),
      bands: step.cleared.map((row) => ({ row, squeeze })),
    });
  }

  /**
   * The cells of `step.preClear` that survive the clear, at `drop` of the way down.
   *
   * Emitted in a stable scan order (column, then bit) so a test can follow one cell
   * across frames — and so a cell's `row` interpolates from where it was to where
   * the plan puts it, which is the collapse.
   */
  private survivingCells(step: Step, drop: number): Array<{ col: number; row: number }> {
    const falls = new Map<number, number>();
    for (const move of step.plan.moves) falls.set(move.from, move.to - move.from);
    const vanish = new Set(step.cleared);
    const cells: Array<{ col: number; row: number }> = [];
    for (let col = 0; col < W; col += 1) {
      for (let bit = 0; bit < H; bit += 1) {
        if ((step.preClear[col] & (1 << bit)) === 0) continue;
        const row = H - 1 - bit;
        if (vanish.has(row)) continue;
        cells.push({ col, row: row + (falls.get(row) ?? 0) * drop });
      }
    }
    return cells;
  }

  private announce(at?: Position): void {
    const now = at ?? position(this.timeline, this.state, this.scheduler.now());
    const total = this.timeline.steps.length;
    const k = Math.min(now.k, total);
    const step = k > 0 && k <= total ? this.timeline.steps[k - 1] : null;
    const stepMs = msPerMove(this.state.mps);
    this.announced = now.k;
    this.onStatus({
      played: k,
      total,
      lines: k === 0 ? 0 : (step?.linesAfter ?? 0),
      landingRow: step ? Math.max(...step.restCells.map((cell) => cell.row)) : null,
      playing: this.state.playing,
      atEnd: k >= total,
      speed: this.state.mps,
      fraction: k >= total ? 1 : Math.max(0, Math.min(1, now.into / stepMs)),
    });
  }
}

export { unpackMove };
