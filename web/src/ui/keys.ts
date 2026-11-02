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
//   * **A focused field or button keeps its keys.** Space would otherwise type a
//     space or click the button a second time, and the arrow keys drive a focused
//     slider by themselves.
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

function takesOwnKeys(target: unknown): boolean {
  if (!target || typeof target !== "object") return false;
  const element = target as { tagName?: unknown; isContentEditable?: unknown };
  if (element.isContentEditable === true) return true;
  const tag = typeof element.tagName === "string" ? element.tagName.toUpperCase() : "";
  return OWNS_KEYS.has(tag);
}

/** The intent behind a key event, or `null` when the page should leave it alone. */
export function resolveKey(event: KeyEventLike): Action | null {
  // A chord belongs to the browser. Shift alone is not a chord.
  if (event.ctrlKey || event.metaKey || event.altKey) return null;
  if (takesOwnKeys(event.target)) return null;

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
  { keys: "?", does: "this list" },
  { keys: "Esc", does: "close" },
];
