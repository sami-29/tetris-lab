// One trailing timer for the commit tier: a drag is 90–180 events and a solve is
// not free.
//
// The measured arithmetic (notes 13 §0.5, §3.2): a 1.5 s drag is 90 `input` events
// at 60 Hz and 180 at 120 Hz, and a cap-width solve costs 0.1–0.5 s. Undebounced
// that is half a minute of queued worker time, and the worker cannot be preempted —
// its message handler runs `gen_game` and `solve` synchronously, so a second
// request simply waits. At 150 ms the same drag fires at most ten solves, and 150 ms
// is still shorter than the time it takes a hand to leave the mouse.
//
// The tiers are the point:
//
//   * **display**, every `input` event, synchronous: the field's number, the
//     readout, the tick. The page never lags the thumb.
//   * **commit**, this file: `input` restarts the timer; `change`, `pointerup`,
//     `keyup` and `blur` flush it, so a click-release or an arrow-key step solves
//     at once and a keyboard visitor is never made to wait out a debounce they did
//     not ask for.
//
// Only the latest value is kept: the intermediate values of a drag are never
// solved, because nobody watches the frames in between. The clock is injected so
// `node --test` owns it, exactly as `ui/playback.ts` injects its scheduler.

/** The timer functions, injectable so a test can advance time by hand. */
export interface Timers {
  setTimer(handler: () => void, ms: number): TimerHandle;
  clearTimer(handle: TimerHandle): void;
}

/**
 * Whatever a timer implementation hands back: a numeric id in the DOM, an object
 * in Node. Named, so the coalescer never traffics in `unknown`.
 */
export type TimerHandle = number | object;

/** 150 ms: ten solves for a 1.5 s drag, and the last one starts before the hand stops. */
export const COMMIT_DELAY_MS = 150;

export interface CoalescerOptions<T> {
  /** Trailing delay in ms. Defaults to `COMMIT_DELAY_MS`. */
  delayMs?: number;
  /** Defaults to the platform's timers. */
  setTimer?: (handler: () => void, ms: number) => TimerHandle;
  clearTimer?: (handle: TimerHandle) => void;
  /** Called with the latest value, once per commit. */
  onFire(value: T): void;
}

export interface Coalescer<T> {
  /** Record a value and restart the timer. Never fires synchronously. */
  push(value: T): void;
  /** Fire the pending value now. Nothing pending is not an error. */
  flush(): void;
  /** Drop the pending value without firing. */
  cancel(): void;
  /** The value that would fire, or `null`. */
  pending(): T | null;
}

interface Box<T> {
  value: T;
}

export function createCoalescer<T>(options: CoalescerOptions<T>): Coalescer<T> {
  const delayMs = options.delayMs ?? COMMIT_DELAY_MS;
  const setTimer =
    options.setTimer ??
    ((handler: () => void, ms: number): TimerHandle => globalThis.setTimeout(handler, ms));
  const clearTimer =
    options.clearTimer ??
    ((handle: TimerHandle): void => globalThis.clearTimeout(handle as number));

  let queued: Box<T> | null = null;
  let handle: TimerHandle | null = null;

  const stop = (): void => {
    if (handle !== null) clearTimer(handle);
    handle = null;
  };

  const fire = (): void => {
    if (queued === null) return;
    const { value } = queued;
    queued = null;
    options.onFire(value);
  };

  return {
    push(value) {
      queued = { value };
      stop();
      handle = setTimer(() => {
        handle = null;
        fire();
      }, delayMs);
    },
    flush() {
      stop();
      fire();
    },
    cancel() {
      stop();
      queued = null;
    },
    pending() {
      return queued === null ? null : queued.value;
    },
  };
}
