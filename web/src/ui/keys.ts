// The keyboard map, as one table.
//
// `resolveKey` turns a key event into an intent and nothing else — no playback
// state, no side effects — so the map is testable without a browser and there is
// exactly one place that knows which keys the page takes. The page stays the only
// place that acts on an intent.
//
// Three rules keep the map from stealing keys that belong to someone else:
//
//   * **No `Ctrl`/`Meta`/`Alt`.** Those chords are the browser's (find, reload,
//     zoom); intercepting them would look like a broken page. Shift is the page's
//     only modifier, and only where the map names it.
//   * **A focused field keeps its keys.** Space would otherwise type a space, and the
//     arrow keys drive a focused slider by themselves. A focused button owns Space
//     and the arrows too — but not the drawer's keys, or the control that opens the
//     drawer would make `E` dead the moment it had been clicked (ticket 16).
//   * **`Escape` only closes.** It never means anything else, because it is the one
//     key a visitor expects to get out of an overlay.
//
// The map is discoverable in three places (§2's status bar, §2.1.2's tooltip rule):
// the chips in the status bar, the `title` on every control, and the `?` sheet.

/** What a key means, as a value the page can act on. */
export type Action =
  | { type: "toggle" }
  /** Move the playhead by `delta` Moves (negative is backwards). */
  | { type: "nudge"; delta: number }
  | { type: "edge"; to: "start" | "end" }
  /** One stop up or down the speed lattice. */
  | { type: "speed"; direction: -1 | 1 }
  | { type: "restart" }
  /** Open the keyboard-map sheet. */
  | { type: "keys" }
  /** Open (or close) the Explainer drawer — the same action as `How it works`. */
  | { type: "explainer" }
  | { type: "close" };

export interface KeyEventLike {
  key: string;
  shiftKey?: boolean;
  ctrlKey?: boolean;
  metaKey?: boolean;
  altKey?: boolean;
  /**
   * The event's target. `unknown`, because a real `KeyboardEvent` carries an
   * `EventTarget` here; the shape below is the only part this module reads.
   */
  target?: unknown;
}

/** Elements that own their keystrokes. */
const OWNS_KEYS = new Set(["INPUT", "TEXTAREA", "SELECT", "BUTTON"]);

/**
 * Keys a focused control still keeps, in the glossary's terms.
 *
 * A `<button>` owns Space and the arrows — they are how it is pressed and moved
 * between. It does **not** own `E`, `?` or `Escape`: after a click the `How it works`
 * control holds focus, and if a button swallowed the drawer's keys then `E` would be
 * dead exactly when a visitor tries it (ticket 16: the drawer closes from either).
 */
const BUTTON_OWNS = new Set([" ", "Spacebar", "ArrowLeft", "ArrowRight", "Home", "End"]);

function takesOwnKeys(target: unknown, key: string): boolean {
  if (!target || typeof target !== "object") return false;
  const element = target as { tagName?: unknown; isContentEditable?: unknown };
  if (element.isContentEditable === true) return true;
  const tag = typeof element.tagName === "string" ? element.tagName.toUpperCase() : "";
  if (tag === "BUTTON") return BUTTON_OWNS.has(key);
  return OWNS_KEYS.has(tag);
}

/** The intent behind a key event, or `null` when the page should leave it alone. */
export function resolveKey(event: KeyEventLike): Action | null {
  // A chord belongs to the browser. Shift alone is not a chord.
  if (event.ctrlKey || event.metaKey || event.altKey) return null;
  if (takesOwnKeys(event.target, event.key)) return null;

  const shift = event.shiftKey === true;
  switch (event.key) {
    case " ":
    case "Spacebar":
      return { type: "toggle" };
    case "ArrowLeft":
      return { type: "nudge", delta: shift ? -10 : -1 };
    case "ArrowRight":
      return { type: "nudge", delta: shift ? 10 : 1 };
    case "Home":
      return { type: "edge", to: "start" };
    case "End":
      return { type: "edge", to: "end" };
    case "-":
    case "_":
      return { type: "speed", direction: -1 };
    case "=":
    case "+":
      return { type: "speed", direction: 1 };
    case "r":
    case "R":
      return { type: "restart" };
    case "?":
      return { type: "keys" };
    case "e":
    case "E":
      // §2: "a drawer behind `How it works` (open with `E`)". A bare letter, no
      // modifier, and the guard above keeps it out of a focused field.
      return { type: "explainer" };
    case "Escape":
      return { type: "close" };
    default:
      return null;
  }
}

/** One stop of the speed keys: the slider moves by this ratio on the log scale. */
export const SPEED_STOP = 1.25;

/** The map, for the status bar's chips and the `?` sheet. */
export const KEY_MAP: ReadonlyArray<{ keys: string; does: string }> = [
  { keys: "Space", does: "play / pause" },
  { keys: "← →", does: "previous / next Move" },
  { keys: "Shift ← →", does: "back / forward 10 Moves" },
  { keys: "Home End", does: "first / last Move" },
  { keys: "- =", does: "slower / faster" },
  { keys: "R", does: "restart" },
  { keys: "E", does: "open or close the explainer" },
  { keys: "?", does: "this list" },
  { keys: "Esc", does: "close" },
];
