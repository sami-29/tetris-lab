// Playback: walk the engine's Move list one piece at a time, dropping each one so
// it lands, and let the rows it completes collapse before the next piece spawns.
//
// The Move words, the Lines and the outcome are the engine's. What playback adds is
// time: a drop of `fallMs` with an accelerating fall, a beat of rest where the
// piece touches down, and a flash on the rows about to go. Motion is hand-written
// on `requestAnimationFrame` — no easing library, no runtime dependency.
//
// The clock and the frame source are injectable so this file can be driven from
// `node --test` with a fake one: `web/test/playback.test.mjs` plays a real game
// through it and asserts the field it ends on is the field the engine's Move list
// describes. Without that, "drops that land" would only ever be checked by eye.

import type { GameMessage } from "../core/protocol.ts";
import { H, clearFullRows, fieldFrom, fullRows, restShift, type Field, type Shape } from "./field.ts";
import { rotationOf } from "./pieces.ts";
import type { Overlay } from "./board.ts";

export interface PlaybackStatus {
  /** How many moves have been played. */
  played: number;
  total: number;
  /** Lines cleared by the replay so far. */
  lines: number;
  /** The board row (0 = top) the piece in flight would land on. */
  landingRow: number | null;
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
  /** Time for one piece to fall from the spawn row to its landing row. */
  fallMs?: number;
  /** How long a landed piece rests before it locks. */
  lockMs?: number;
  /** How long completed rows stay lit before they collapse. */
  clearMs?: number;
  /** How long the finished board is held before playback starts over. */
  holdMs?: number;
  scheduler?: Scheduler;
  onStatus?(status: PlaybackStatus): void;
}

interface Timing {
  fallMs: number;
  lockMs: number;
  clearMs: number;
  holdMs: number;
}

/** §4.1.1: `(piece_index << 16) | (rotation << 8) | column`. */
export function unpackMove(word: number): { piece: number; rotation: number; column: number } {
  return { piece: (word >>> 16) & 0xff, rotation: (word >>> 8) & 0xff, column: word & 0xff };
}

export class Playback {
  private readonly view: RenderTarget;
  private readonly moves: Uint32Array;
  private readonly initial: Field;
  private readonly timing: Timing;
  private readonly scheduler: Scheduler;
  private readonly onStatus: (status: PlaybackStatus) => void;

  private field: Field;
  private lines = 0;
  private index = 0;
  private phase: "fall" | "lock" | "clear" | "hold" = "fall";
  private phaseStart = 0;
  private shiftFrom = 0;
  private shiftTo = 0;
  private shape: Shape | null = null;
  private column = 0;
  private flash = 0;
  private running = false;
  private handle = 0;

  constructor(view: RenderTarget, game: GameMessage, options: PlaybackOptions = {}) {
    this.view = view;
    this.moves = game.moves;
    this.initial = fieldFrom(game.board);
    this.field = fieldFrom(game.board);
    this.timing = {
      fallMs: options.fallMs ?? 150,
      lockMs: options.lockMs ?? 70,
      clearMs: options.clearMs ?? 160,
      holdMs: options.holdMs ?? 2200,
    };
    this.scheduler = options.scheduler ?? browserScheduler;
    this.onStatus = options.onStatus ?? (() => {});
  }

  /** Start from the beginning, whatever state it was in. */
  start(): void {
    if (this.running) return;
    this.running = true;
    this.reset();
    this.phaseStart = this.scheduler.now();
    this.handle = this.scheduler.frame(this.tick);
  }

  stop(): void {
    this.running = false;
    if (this.handle !== 0) this.scheduler.cancel(this.handle);
    this.handle = 0;
  }

  private reset(): void {
    this.field = fieldFrom(this.initial);
    this.lines = 0;
    this.index = 0;
    this.flash = 0;
    this.shape = null;
    this.enterFall();
  }

  /**
   * Set up the drop of the move at `index`; false when the list is done.
   *
   * The resting shift comes from `restShift`, the same rule the engine drops by, so
   * a piece lands exactly where the engine put it — including when a column has a
   * hole a piece must not tunnel into.
   */
  private enterFall(): boolean {
    const word = this.moves[this.index];
    if (word === undefined) return false;
    const move = unpackMove(word);
    const shape = rotationOf(move.piece, move.rotation);
    if (!shape) return false;
    const landing = restShift(this.field, shape, move.column);
    if (landing === null) return false;

    this.shape = shape;
    this.column = move.column;
    // Spawn at row 0, then fall to the resting shift. `restShift` never returns a
    // shift above the spawn row, so the drop always travels downward.
    this.shiftFrom = H - shape.height;
    this.shiftTo = landing;
    this.phase = "fall";
    this.announce();
    return true;
  }

  private readonly tick = (now: number): void => {
    if (!this.running) return;
    const elapsed = now - this.phaseStart;

    switch (this.phase) {
      case "fall": {
        const t = Math.min(1, elapsed / this.timing.fallMs);
        const eased = t * t;
        const shift = this.shiftFrom + (this.shiftTo - this.shiftFrom) * eased;
        this.render(shift);
        if (t >= 1) {
          this.phase = "lock";
          this.phaseStart = now;
        }
        break;
      }

      case "lock": {
        this.render(this.shiftTo);
        if (elapsed >= this.timing.lockMs) this.commit(now);
        break;
      }

      case "clear": {
        this.render(null);
        if (elapsed >= this.timing.clearMs) {
          this.lines += clearFullRows(this.field);
          this.flash = 0;
          this.index += 1;
          if (!this.enterFall()) this.phase = "hold";
          this.phaseStart = now;
          this.announce();
        }
        break;
      }

      case "hold": {
        this.render(null);
        if (elapsed >= this.timing.holdMs) {
          this.reset();
          this.phaseStart = now;
          this.announce();
        }
        break;
      }
    }

    this.handle = this.scheduler.frame(this.tick);
  };

  /** The piece has touched down: stamp it, and light the rows it completed. */
  private commit(now: number): void {
    const shape = this.shape;
    if (!shape) {
      this.phase = "hold";
      this.phaseStart = now;
      return;
    }
    for (let i = 0; i < shape.width; i += 1) {
      const col = this.column + i;
      this.field[col] = (this.field[col] | (shape.cols[i] << this.shiftTo)) >>> 0;
    }
    const full = fullRows(this.field);
    this.phaseStart = now;
    if (full === 0) {
      this.index += 1;
      if (this.enterFall()) {
        this.phaseStart = now;
      } else {
        this.phase = "hold";
      }
    } else {
      this.flash = full;
      this.phase = "clear";
    }
    this.announce();
  }

  /** Draw the settled field plus the piece in flight at `shift` (null: resting). */
  private render(shift: number | null): void {
    const clearRows: number[] = [];
    for (let bit = 0; bit < H; bit += 1) {
      if ((this.flash & (1 << bit)) !== 0) clearRows.push(H - 1 - bit);
    }

    const shape = this.shape;
    if (!shape || shift === null) {
      this.view.draw(this.field, { clearRows });
      return;
    }

    const cellsOf = (at: number): Array<{ col: number; row: number }> => {
      const cells: Array<{ col: number; row: number }> = [];
      for (let i = 0; i < shape.width; i += 1) {
        for (let offset = 0; offset < shape.height; offset += 1) {
          if ((shape.cols[i] & (1 << offset)) !== 0) {
            cells.push({ col: this.column + i, row: H - 1 - (at + offset) });
          }
        }
      }
      return cells;
    };

    this.view.draw(this.field, {
      piece: cellsOf(shift),
      ghost: cellsOf(this.shiftTo),
      clearRows,
    });
  }

  private announce(): void {
    this.onStatus({
      played: this.index,
      total: this.moves.length,
      lines: this.lines,
      landingRow: this.shape ? H - 1 - this.shiftTo : null,
    });
  }
}
