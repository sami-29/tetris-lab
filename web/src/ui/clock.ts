// The playback clock: `(k, into)` as a pure state machine.
//
// There is no animation object here and nothing in flight to cancel. The state is
// "which Move is on screen, how far into it, and when that was true"; the picture
// is `reconstruct(k)` plus that fraction. Which is why every interaction is the
// same operation:
//
//   * **pause** folds the position at `now` into `(k, into)` and stops the clock —
//     the board keeps the interpolated pose instead of snapping to a step edge;
//   * **resume** re-anchors at `now` with the frozen fraction, so a drop continues
//     from where it froze;
//   * **seek** sets `k` and stops: the next frame draws `reconstruct(k)`, and the
//     step after a seek is applied *to that board*, because a step's `preBoard` is
//     `reconstruct(k)` by construction;
//   * **setSpeed** converts the fraction, so changing speed mid-fall does not
//     teleport the piece.
//
// It is time-based, not frame-based: `k` is elapsed time over step time, so the
// same wall clock gives the same position at 30, 60 or 120 Hz. A frame longer than
// a step samples the timeline later rather than tearing, and a tab that was hidden
// for a minute advances at most `STALL_MS` worth of Moves instead of replaying the
// minute.

import { MPS_DEFAULT, clampMps, msPerMove } from "./phases.ts";
import type { Step, Timeline } from "./timeline.ts";

/**
 * The most time one tick may spend. A `requestAnimationFrame` callback after a
 * hidden tab reports a huge delta; clamping it keeps the page at a human pace
 * instead of fast-forwarding to the end of the game.
 */
export const STALL_MS = 250;

export interface PlayState {
  playing: boolean;
  /** Moves per second. */
  mps: number;
  /** How many Moves are behind the playhead. `k = 0` is the starting Board. */
  k: number;
  /** How far into step `k + 1`, in milliseconds. */
  into: number;
  /** The timestamp `(k, into)` was true at. */
  t: number;
}

/** Where the playhead is at `now`, without changing anything. */
export interface Position {
  k: number;
  into: number;
  /** `k` has reached the end of the Move list: there is nothing left to play. */
  atEnd: boolean;
  playing: boolean;
}

export function initialState(mps: number = MPS_DEFAULT, t: number = 0): PlayState {
  return { playing: false, mps: clampMps(mps), k: 0, into: 0, t };
}

/** The step that is playing at `k`, or `null` at the start and past the end. */
export function stepAt(timeline: Timeline, k: number): Step | null {
  if (!Number.isFinite(k)) return null;
  const index = Math.trunc(k);
  if (index < 1 || index > timeline.steps.length) return null;
  return timeline.steps[index - 1];
}

/** Where the playhead is at `now`; a pure function of the state and the clock. */
export function position(timeline: Timeline, state: PlayState, now: number): Position {
  const total = timeline.steps.length;
  if (!state.playing) {
    const k = Math.max(0, Math.min(total, state.k));
    return { k, into: k === total ? 0 : state.into, atEnd: k === total, playing: false };
  }
  const stepMs = msPerMove(state.mps);
  const elapsed = Math.max(0, Math.min(STALL_MS, now - state.t));
  const total_ = state.into + elapsed;
  const advance = Math.floor(total_ / stepMs);
  const k = Math.min(total, state.k + advance);
  if (k === total) return { k, into: 0, atEnd: true, playing: true };
  return { k, into: total_ - advance * stepMs, atEnd: false, playing: true };
}

/**
 * Fold the position at `now` back into the state, so the clock keeps running from
 * the same place. The page calls this once per frame; it is also what drops the
 * excess of a stalled frame for good rather than banking it.
 */
export function reanchor(timeline: Timeline, state: PlayState, now: number): PlayState {
  const at = position(timeline, state, now);
  return { playing: state.playing, mps: state.mps, k: at.k, into: at.into, t: now };
}

/** Stop where we are, keeping the pose the last frame drew. */
export function pause(timeline: Timeline, state: PlayState, now: number): PlayState {
  const at = position(timeline, state, now);
  return { playing: false, mps: state.mps, k: at.k, into: at.into, t: now };
}

/** Start the clock again from the frozen (or scrubbed) position. */
export function resume(timeline: Timeline, state: PlayState, now: number): PlayState {
  const at = position(timeline, state, now);
  return { playing: true, mps: state.mps, k: at.k, into: at.into, t: now };
}

export function toggle(timeline: Timeline, state: PlayState, now: number): PlayState {
  return state.playing ? pause(timeline, state, now) : resume(timeline, state, now);
}

/**
 * Move the playhead to `k` and stop. A scrub pauses, always: dragging the strip
 * while the clock runs would fight the visitor's pointer.
 */
export function seek(timeline: Timeline, state: PlayState, k: number, now: number): PlayState {
  const total = timeline.steps.length;
  const target = Number.isFinite(k) ? Math.max(0, Math.min(total, Math.trunc(k))) : 0;
  return { playing: false, mps: state.mps, k: target, into: 0, t: now };
}

/**
 * Change the speed and keep the fraction of the Move that is already played.
 * Without this, dragging the speed slider mid-drop teleports the falling piece.
 */
export function setSpeed(timeline: Timeline, state: PlayState, mps: number, now: number): PlayState {
  const at = position(timeline, state, now);
  const before = msPerMove(state.mps);
  const after = msPerMove(mps);
  const fraction = at.k >= timeline.steps.length ? 0 : at.into / before;
  return {
    playing: state.playing,
    mps: clampMps(mps),
    k: at.k,
    into: Math.max(0, Math.min(after, fraction * after)),
    t: now,
  };
}

/**
 * Move 0, playing, at the speed the visitor chose.
 *
 * The timeline is in the signature because every action here takes it, so a caller
 * (the page or a test) can apply any intent without knowing which ones need it.
 */
export function restart(_timeline: Timeline, state: PlayState, now: number): PlayState {
  return { playing: true, mps: state.mps, k: 0, into: 0, t: now };
}

/** How far into the Move on screen, `0..1`; 0 at a step edge. */
export function fraction(timeline: Timeline, state: PlayState, now: number): number {
  const at = position(timeline, state, now);
  if (at.atEnd) return 1;
  return at.into / msPerMove(state.mps);
}
