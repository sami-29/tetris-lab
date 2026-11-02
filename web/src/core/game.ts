// The game, as the URL carries it: five integers, parsed, clamped and written
// back. No DOM, no wasm — this module is the whole of §2.2's state contract and it
// is pure so the contract can be tested without a page (`web/test/url.test.mjs`).
//
// §2.2, in three sentences:
//
//   * "Query string carries the settable game — `seed`, `goal`, `pieces`, `cap`,
//     `beam`. Lowercase names; absent or unparseable values fall back to the
//     default rather than erroring."
//   * "Every value accepted from the URL or the Playground is clamped to the
//     generator's domain (§5.8) before it reaches wasm, and `beam` is additionally
//     clamped to `1..WIDTH_CAP`."
//   * "Evaluator weights are **not** in the URL (they are the expert's transient
//     state)."
//
// Two bounds in here are not in §5.8's table and are not invented either:
//
//   * `goal` 1..255 — the packed result carries Lines cleared in bits 41..48
//     (§4.2), so a goal above 255 could never be observed as won. §5.8 omits the
//     row; the ABI fixes the number.
//   * `beam` 1..`widthCap` — the UI bound, narrower than §5.8's 16,384, and read
//     from the measured width curve rather than typed here.
//
// The clamp is why an old link still plays. §5.8: "a URL stored above a re-measured
// cap would start returning status 1 instead of a solve". Clamping once, here,
// means a link written by one build names *a game* on the next, and the control the
// visitor sees shows the value they were given rather than an error.
//
// Vocabulary: `cap` in the URL is the **starting height** (`height_cap`, the third
// argument of `gen_game`), never the width cap — one spec paragraph uses "cap" for
// both, so every name in here says which one it means.

import type { GameInput } from "./protocol.ts";

/** The five query keys, in the order a link emits them (§2.2). */
export type GameField = "seed" | "goal" | "pieces" | "cap" | "beam";

export const URL_KEYS: readonly GameField[] = ["seed", "goal", "pieces", "cap", "beam"];

/**
 * §5.8's Domain, plus the two bounds the contract itself does not carry.
 *
 * §5.8: "The width the site ships at and the slider's maximum are **not** part of
 * this domain: they are a performance measurement (§9.5), so the engine accepts up
 * to 16,384 and the UI clamps to the measured cap." So `beam.max` here is the ABI's
 * ceiling and the UI narrows it further with `Bounds.widthCap`.
 */
export const LIMITS: Record<GameField, { readonly min: number; readonly max: number }> = {
  seed: { min: 0, max: 4294967295 },
  goal: { min: 1, max: 255 },
  pieces: { min: 1, max: 200 },
  cap: { min: 2, max: 19 },
  beam: { min: 1, max: 16384 },
};

/** The two numbers the page reads from the committed width curve (§9.5). */
export interface Bounds {
  /** `DEFAULT_BEAM`: the width the toolbar opens at, and the URL's default. */
  readonly defaultBeam: number;
  /** `WIDTH_CAP`: the slider's maximum and the URL's `beam` ceiling. */
  readonly widthCap: number;
}

/** URL key ⇄ `GameInput` field. `cap` is the starting height; `beam` is the width. */
const FIELD_OF: Record<GameField, keyof GameInput> = {
  seed: "seed",
  goal: "goal",
  pieces: "pieces",
  cap: "heightCap",
  beam: "beamWidth",
};

/**
 * The value a field falls back to: §2.2's default game, with the width from the
 * artifact. Not a literal for `beam` — a re-measured default moves the URL default
 * and the slider's opening position together (§9.5).
 */
export function defaultValue(field: GameField, bounds: Bounds): number {
  switch (field) {
    case "seed":
      return 0;
    case "goal":
      return 8;
    case "pieces":
      return 40;
    case "cap":
      return 7;
    case "beam":
      return bounds.defaultBeam;
  }
}

/** The interval a field's value is kept inside: §5.8's, narrowed where the UI is. */
export function boundOf(field: GameField, bounds: Bounds): { min: number; max: number } {
  const limit = LIMITS[field];
  if (field !== "beam") return { min: limit.min, max: limit.max };
  // The cap is a time bound, not a correctness one: never wider than §5.8, never
  // narrower than the single width the engine can always accept.
  const max = Math.max(limit.min, Math.min(limit.max, bounds.widthCap));
  return { min: limit.min, max };
}

/** §2.2's default game: seed 0, goal 8, 40 pieces, starting height 7, `DEFAULT_BEAM`. */
export function defaultGame(bounds: Bounds): GameInput {
  return fromFields({
    seed: defaultValue("seed", bounds),
    goal: defaultValue("goal", bounds),
    pieces: defaultValue("pieces", bounds),
    cap: defaultValue("cap", bounds),
    beam: defaultValue("beam", bounds),
  });
}

/** One game as its five URL values. */
export function fieldsOf(game: GameInput): Record<GameField, number> {
  return {
    seed: game.seed,
    goal: game.goal,
    pieces: game.pieces,
    cap: game.heightCap,
    beam: game.beamWidth,
  };
}

/** The inverse of `fieldsOf`. */
function fromFields(fields: Record<GameField, number>): GameInput {
  return {
    seed: fields.seed,
    goal: fields.goal,
    pieces: fields.pieces,
    heightCap: fields.cap,
    beamWidth: fields.beam,
  };
}

/**
 * A whole decimal integer, or `null` for "absent or unparseable".
 *
 * The rule is trim, then `/^[+-]?[0-9]+$/`, and nothing looser. `Number()` alone
 * would accept `0x10`, `1e3` and `7.5`; `parseInt()` alone would accept `40abc` and
 * silently truncate `7.5`. Both are ways for a URL to name a game the visitor did
 * not type.
 */
export function parseInteger(raw: string | null): number | null {
  if (raw === null) return null;
  const text = raw.trim();
  if (!/^[+-]?[0-9]+$/.test(text)) return null;
  const value = Number(text);
  return Number.isFinite(value) ? value : null;
}

/** One value, clamped into the field's interval. `null` is the default. */
export function clampValue(field: GameField, value: number | null, bounds: Bounds): number {
  const { min, max } = boundOf(field, bounds);
  const wanted = value === null || !Number.isInteger(value) ? defaultValue(field, bounds) : value;
  return Math.min(max, Math.max(min, wanted));
}

/**
 * Total: any partial game becomes a complete one, every field inside the Domain.
 * A field that is absent or not an integer falls back to its default; a field that
 * is an integer outside the Domain is clamped.
 */
export function clampGame(input: Partial<GameInput>, bounds: Bounds): GameInput {
  const out: Record<GameField, number> = {
    seed: defaultValue("seed", bounds),
    goal: defaultValue("goal", bounds),
    pieces: defaultValue("pieces", bounds),
    cap: defaultValue("cap", bounds),
    beam: defaultValue("beam", bounds),
  };
  for (const field of URL_KEYS) {
    const raw = input[FIELD_OF[field]];
    out[field] = clampValue(field, typeof raw === "number" ? raw : null, bounds);
  }
  return fromFields(out);
}

/**
 * The game a query string names. Every absent field is the default, every
 * unparseable one is the default, every out-of-domain one is clamped.
 *
 * A repeated key takes the first value — what `URLSearchParams.get` does, and what
 * the serialiser below never emits twice. Keys are case-sensitive, so `?Cap=12` is
 * an absent `cap`; unknown keys are inert, which is what keeps a tracking
 * parameter or a stale `?weights=` from meaning anything.
 */
export function parseGame(search: string, bounds: Bounds): GameInput {
  const params = new URLSearchParams(search);
  const fields: Record<GameField, number> = {
    seed: 0,
    goal: 0,
    pieces: 0,
    cap: 0,
    beam: 0,
  };
  for (const field of URL_KEYS) {
    fields[field] = clampValue(field, parseInteger(params.get(field)), bounds);
  }
  return fromFields(fields);
}

/**
 * The query string that carries a game, in `seed, goal, pieces, cap, beam` order.
 *
 * A key equal to the default is left out, so a bare URL stays bare and "a link to
 * the Lab" stays distinguishable from "a link to a game". It also means
 * `parseGame(serialiseGame(game))` is the game — the reload criterion — and that
 * writing the URL normalises anything hand-edited out of it.
 *
 * The five keys are iterated from `URL_KEYS` over a five-field game: there is no
 * branch here that could reach a sixth name, which is the structural half of
 * "Evaluator weights are not in the URL".
 */
export function serialiseGame(game: GameInput, bounds: Bounds): string {
  const fields = fieldsOf(game);
  const params = new URLSearchParams();
  for (const field of URL_KEYS) {
    const value = clampValue(field, fields[field], bounds);
    if (value !== defaultValue(field, bounds)) params.set(field, String(value));
  }
  const query = params.toString();
  return query === "" ? "" : `?${query}`;
}

/** One control moved: only that field's value changes. */
export function applyField(
  game: GameInput,
  field: GameField,
  raw: string,
  bounds: Bounds,
): GameInput {
  const fields = fieldsOf(game);
  fields[field] = clampValue(field, parseInteger(raw), bounds);
  return fromFields(fields);
}

/** Do two games name the same solve? The five values are the whole game. */
export function sameGame(a: GameInput, b: GameInput): boolean {
  return reSolveReason(a, b) === "none";
}

/**
 * Which control asks for a new Solve, or `"none"`.
 *
 * A re-solve is a difference in one of the five fields, and nothing else: playback
 * speed, seek position, the tree bound and the selected state are view state and
 * never reach this function. `current === null` is the first solve, not a re-solve,
 * so the caller can distinguish the two by passing `null` once.
 *
 * Note the two directions §2.2 separates and code must not confuse: **in the URL**
 * is not the same as **causes a solve**. Evaluator Weights cause a solve and are
 * never in the URL; a playback speed is in neither.
 */
export function reSolveReason(next: GameInput, current: GameInput | null): GameField | "none" {
  if (current === null) return "none";
  const a = fieldsOf(next);
  const b = fieldsOf(current);
  for (const field of URL_KEYS) {
    if (a[field] !== b[field]) return field;
  }
  return "none";
}

/** The game in words, for the status line: every value labelled, none implied. */
export function describeGame(game: GameInput): string {
  return (
    `seed ${game.seed} · goal ${game.goal} lines · ${game.pieces} pieces` +
    ` · starting height ${game.heightCap} · beam ${game.beamWidth}`
  );
}
