// Speed and the phase budget: how long a Move takes, and how that time is spent.
//
// Two decisions, both pure arithmetic, both testable without a browser:
//
//   * **Speed is continuous.** The control is Moves per second, mapped through a
//     log scale so the slow end (where a visitor watches one placement) gets as
//     much slider travel as the fast end. There are no presets anywhere: the four
//     old multipliers produced four different animations, which is exactly what the
//     ticket asks to be rid of.
//   * **The phases are shares of the step, never a table of milliseconds.** A drop
//     is 45 % of the Move, the clear is `0.26 + 0.10·√rows` (the gravity rule —
//     four rows are heavier than one, but not four times slower), and the rest is a
//     beat of rest, which is what makes a step read as a *step* rather than a
//     continuous slide. Change the speed and every phase retimes with it; that is
//     the same animation faster, not a different animation.
//
// When the step is shorter than the screen can draw (a step under one frame), the
// plan says so (`mode: "skip"`) instead of pretending: the renderer draws the
// finished position and the move strip carries the feedback. Between one and three
// frames a step (`mode: "compact"`) the plan drops the rest beat and splits the
// step between drop and clear so nothing is left half-drawn.

/** The slow end: four seconds a Move, slow enough to read one placement. */
export const MPS_MIN = 0.25;
/**
 * The fast end: 25 ms a Move, chosen so a step is never shorter than a 60 Hz frame
 * (measured 1.5 frames per step at 40 mps). Faster than this is fast-forward, not
 * speed: Moves would be skipped outright.
 */
export const MPS_MAX = 40;
/** The prototype's "measured pace", so the default feel is the one that shipped. */
export const MPS_DEFAULT = 1.43;
/** One 60 Hz frame, the default sampling assumption. */
export const FRAME_MS = 1000 / 60;

export type PhaseMode = "full" | "compact" | "skip";

export interface PhasePlan {
  mode: PhaseMode;
  /** The whole Move. */
  stepMs: number;
  /** Piece falling from the spawn row to its resting shift. */
  dropMs: number;
  /** The collapse the placement caused, if any. */
  clearMs: number;
  /** The first part of the clear: the cleared bands squeeze away. */
  contractMs: number;
  /** The second part: the surviving runs fall into the gap. */
  fallMs: number;
  /** The board sitting still at the end of the step. */
  holdMs: number;
}

/** Clamp to the page's range; a nonsense value is the default, not a divide by zero. */
export function clampMps(mps: number): number {
  if (!Number.isFinite(mps)) return MPS_DEFAULT;
  return Math.min(MPS_MAX, Math.max(MPS_MIN, mps));
}

/** Milliseconds per Move at a speed in Moves per second. */
export function msPerMove(mps: number): number {
  return 1000 / clampMps(mps);
}

/** A slider position in `[0, 1]` as a speed: logarithmic, so equal steps are equal ratios. */
export function sliderToMps(position: number): number {
  const t = !Number.isFinite(position) ? 0 : Math.min(1, Math.max(0, position));
  if (t <= 0) return MPS_MIN;
  if (t >= 1) return MPS_MAX;
  return Math.exp(Math.log(MPS_MIN) + t * (Math.log(MPS_MAX) - Math.log(MPS_MIN)));
}

/** The inverse: where the slider sits for a speed. */
export function mpsToSlider(mps: number): number {
  const clamped = clampMps(mps);
  if (clamped <= MPS_MIN) return 0;
  if (clamped >= MPS_MAX) return 1;
  return (Math.log(clamped) - Math.log(MPS_MIN)) / (Math.log(MPS_MAX) - Math.log(MPS_MIN));
}

const DROP_SHARE = 0.45;
/** The most of a step the drop and the clear may take, so a rest beat always remains. */
const MOTION_BUDGET = 0.95;
const COMPACT_DROP_SHARE = 11 / 20;
const COMPACT_CLEAR_SHARE = 9 / 20;
/** Below three frames a step, the rest beat is dropped rather than squeezed. */
const COMPACT_FRAMES = 3;

/**
 * How one Move's time is spent when it clears `clearedRows` rows.
 *
 * `frameMs` is the display's frame time: it decides only whether the animation is
 * possible at all, never how it looks.
 */
export function phases(stepMs: number, clearedRows: number, frameMs: number = FRAME_MS): PhasePlan {
  const step = Number.isFinite(stepMs) && stepMs > 0 ? stepMs : 0;
  const frame = Number.isFinite(frameMs) && frameMs > 0 ? frameMs : FRAME_MS;
  const rows = Math.max(0, Math.min(4, Math.trunc(clearedRows) || 0));

  if (step < frame) {
    // A step shorter than a frame cannot be drawn: the finished position it is.
    return { mode: "skip", stepMs: step, dropMs: 0, clearMs: 0, contractMs: 0, fallMs: 0, holdMs: step };
  }

  let dropShare: number;
  let clearShare: number;
  const compact = step < COMPACT_FRAMES * frame;
  if (compact) {
    dropShare = COMPACT_DROP_SHARE;
    clearShare = rows > 0 ? COMPACT_CLEAR_SHARE : 0;
  } else {
    dropShare = DROP_SHARE;
    // t ∝ √d: a four-row clear gets 1.28x the time of a one-row clear, so the
    // stack visibly falls four cells in a little more time — heavier, not slower.
    clearShare = rows > 0 ? 0.26 + 0.1 * Math.sqrt(rows) : 0;
    const total = dropShare + clearShare;
    if (total > MOTION_BUDGET) {
      dropShare *= MOTION_BUDGET / total;
      clearShare *= MOTION_BUDGET / total;
    }
  }

  const dropMs = dropShare * step;
  const clearMs = clearShare * step;
  const contractMs = clearMs * 0.3;
  const fallMs = clearMs - contractMs;
  const holdMs = Math.max(0, step - dropMs - clearMs);

  return {
    mode: compact ? "compact" : "full",
    stepMs: step,
    dropMs,
    clearMs,
    contractMs,
    fallMs,
    holdMs,
  };
}
