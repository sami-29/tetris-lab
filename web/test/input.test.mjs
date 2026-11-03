// The move strip as a seek bar, and the keyboard map.
//
// Two small pure pieces carry this: `web/src/ui/seek.ts` turns a pointer position
// into a Move index (and back), so any Move is one gesture away, and
// `web/src/ui/keys.ts` turns a key event into an intent, so the map is one table
// rather than scattered listeners. Both are asserted here; the DOM wiring that
// calls them is checked by `node scripts/browser-check.mjs`.

import { test } from "node:test";
import assert from "node:assert/strict";

import { moveToX, stripMode, xToMove } from "../src/ui/seek.ts";
import { resolveKey } from "../src/ui/keys.ts";

const key = (overrides) => ({ key: "x", shiftKey: false, ctrlKey: false, metaKey: false, altKey: false, ...overrides });

test("the strip's ends are the first and last position, exactly", () => {
  assert.equal(xToMove(0, 640, 20), 0, "the left edge is move 0, the starting Board");
  assert.equal(xToMove(640, 640, 20), 20, "the right edge is the last Move");
  assert.equal(xToMove(-50, 640, 20), 0, "past the left edge clamps");
  assert.equal(xToMove(900, 640, 20), 20, "past the right edge clamps");

  // One gesture to any Move: a click in the middle of the strip lands on the
  // nearest boundary, and the mapping never jumps over a Move.
  let last = -1;
  for (let x = 0; x <= 640; x += 1) {
    const k = xToMove(x, 640, 20);
    assert.ok(k >= last, "the mapping is monotone");
    assert.ok(k - last <= 1, "and never skips a Move in one pixel");
    last = k;
  }
  assert.equal(xToMove(16, 640, 20), 1, "a cell's worth of pixels is a Move");

  // Every Move is reachable, and `moveToX` is its inverse at cell centres.
  for (let k = 0; k <= 20; k += 1) {
    assert.equal(xToMove(moveToX(k, 640, 20), 640, 20), k, `move ${k} round-trips`);
  }
  assert.equal(moveToX(0, 640, 20), 0);
  assert.equal(moveToX(20, 640, 20), 640);

  // Degenerate inputs are not crashes: a hidden strip, a one-Move game, a
  // 200-Move game.
  assert.equal(xToMove(100, 0, 20), 0, "a zero-width strip is move 0");
  assert.equal(moveToX(0, 0, 20), 0);
  assert.equal(xToMove(640, 640, 1), 1, "a one-Move game reaches its only Move");
  assert.equal(xToMove(320, 640, 1), 1, "and no further than it");
  assert.equal(xToMove(608, 640, 200), 190, "a 200-Move game still addresses single Moves");
});

test("the strip drops to ticks when glyphs would not fit", () => {
  assert.equal(stripMode(1), "glyph");
  assert.equal(stripMode(40), "glyph");
  assert.equal(stripMode(60), "glyph");
  assert.equal(stripMode(61), "tick", "past sixty Moves a 640 px strip has no room for glyphs");
  assert.equal(stripMode(200), "tick");
});

test("Space is play/pause, and the keys that scroll are taken deliberately", () => {
  assert.deepEqual(resolveKey(key({ key: " " })), { type: "toggle" });
  assert.deepEqual(resolveKey(key({ key: "Spacebar" })), { type: "toggle" }, "the legacy name too");
  assert.deepEqual(resolveKey(key({ key: "ArrowLeft" })), { type: "nudge", delta: -1 });
  assert.deepEqual(resolveKey(key({ key: "ArrowRight" })), { type: "nudge", delta: 1 });
  assert.deepEqual(resolveKey(key({ key: "ArrowLeft", shiftKey: true })), { type: "nudge", delta: -10 });
  assert.deepEqual(resolveKey(key({ key: "ArrowRight", shiftKey: true })), { type: "nudge", delta: 10 });
  assert.deepEqual(resolveKey(key({ key: "Home" })), { type: "edge", to: "start" });
  assert.deepEqual(resolveKey(key({ key: "End" })), { type: "edge", to: "end" });
  assert.deepEqual(resolveKey(key({ key: "r" })), { type: "restart" });
  assert.deepEqual(resolveKey(key({ key: "R" })), { type: "restart" }, "either case");
  assert.deepEqual(resolveKey(key({ key: "-" })), { type: "speed", direction: -1 });
  assert.deepEqual(resolveKey(key({ key: "_" })), { type: "speed", direction: -1 });
  assert.deepEqual(resolveKey(key({ key: "=" })), { type: "speed", direction: 1 });
  assert.deepEqual(resolveKey(key({ key: "+" })), { type: "speed", direction: 1 });
  assert.deepEqual(resolveKey(key({ key: "?" })), { type: "keys" }, "the map itself is a key");
  // Ticket 14's one new row: §2 says the Explainer drawer opens with `E`, so `E` is
  // in the same table as everything else — not an out-of-band listener.
  assert.deepEqual(resolveKey(key({ key: "e" })), { type: "explainer" });
  assert.deepEqual(resolveKey(key({ key: "E" })), { type: "explainer" }, "either case");
  assert.deepEqual(resolveKey(key({ key: "Escape" })), { type: "close" });
});

test("a modifier chord belongs to the browser, and typing is not a shortcut", () => {
  for (const name of [" ", "ArrowLeft", "ArrowRight", "Home", "End", "r", "e", "-", "=", "?", "Escape"]) {
    assert.equal(resolveKey(key({ key: name, ctrlKey: true })), null, `Ctrl+${name} is not ours`);
    assert.equal(resolveKey(key({ key: name, metaKey: true })), null, `Meta+${name} is not ours`);
    assert.equal(resolveKey(key({ key: name, altKey: true })), null, `Alt+${name} is not ours`);
  }
  // Shift is the page's only modifier, and only where the map says so.
  assert.deepEqual(resolveKey(key({ key: " " })), resolveKey(key({ key: " ", shiftKey: true })), "Shift+Space is Space");

  // A focused field or button keeps its own keys: Space would otherwise type a
  // space or click the button a second time.
  for (const target of [
    { tagName: "INPUT" },
    { tagName: "TEXTAREA" },
    { tagName: "SELECT" },
    { tagName: "DIV", isContentEditable: true },
  ]) {
    for (const name of [" ", "ArrowLeft", "r", "e", "?"])
      assert.equal(resolveKey(key({ key: name, target })), null, `${target.tagName} keeps ${name}`);
  }
  // A button owns Space and the arrows, which are its own keys. It does not own the
  // drawer's: after a click the `How it works` control holds focus, and a focused
  // control must not make `E` dead (ticket 16's "closes from either").
  for (const name of [" ", "ArrowLeft", "r"])
    assert.equal(resolveKey(key({ key: name, target: { tagName: "BUTTON" } })), null, `a button keeps ${name}`);
  for (const name of ["e", "E", "?", "Escape"])
    assert.notEqual(resolveKey(key({ key: name, target: { tagName: "BUTTON" } })), null, `a button does not own ${name}`);
  // A plain element, a canvas and a missing target are all fair game.
  assert.deepEqual(resolveKey(key({ key: " ", target: { tagName: "BODY" } })), { type: "toggle" });
  assert.deepEqual(resolveKey(key({ key: " ", target: null })), { type: "toggle" });
  assert.deepEqual(resolveKey(key({ key: " ", target: { tagName: "CANVAS" } })), { type: "toggle" });

  // Keys nobody mapped are nobody's: no accidental interception.
  for (const name of ["a", "q", "1", "Tab", "Enter", "F5", "F11", "/", "ArrowUp", "ArrowDown"]) {
    assert.equal(resolveKey(key({ key: name })), null, `${name} is not a shortcut`);
  }
});

test("nudging and edges are clamped by the caller, not silently wrapped", () => {
  // The intents are values, not side effects: the same event always resolves to
  // the same action, and nothing here reads or writes playback state.
  const event = key({ key: "ArrowLeft" });
  assert.deepEqual(resolveKey(event), resolveKey(event));
  assert.deepEqual(resolveKey({ ...event }), { type: "nudge", delta: -1 });
  // The map is a superset of the ticket's five verbs.
  const verbs = new Set(
    [" ", "ArrowLeft", "Home", "r", "-", "?"].map((name) => resolveKey(key({ key: name })).type),
  );
  for (const verb of ["toggle", "nudge", "edge", "restart", "speed", "keys"]) {
    assert.ok(verbs.has(verb), `the map covers ${verb}`);
  }
});
