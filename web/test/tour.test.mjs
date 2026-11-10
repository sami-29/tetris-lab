// The tour's seen flag: read, write, and a browser whose storage is missing or throws.
//
// Everything else the tour does (the ring, the card's docking, focus return, the key
// capture) needs a laid-out page, and `scripts/shell-check.mjs` drives that in a real
// browser. What can be asserted without one is the one thing the tour keeps between
// visits, and that a visitor with blocked storage still gets a working tour.

import { test, afterEach } from "node:test";
import assert from "node:assert/strict";

import { SEEN_KEY, markSeen, tourSeen } from "../src/ui/tour.ts";

const original = Object.getOwnPropertyDescriptor(globalThis, "localStorage");

function install(storage) {
  Object.defineProperty(globalThis, "localStorage", { value: storage, configurable: true, writable: true });
}

/** A storage that keeps what it is given, like a browser's. */
function memory() {
  const map = new Map();
  return {
    map,
    getItem: (key) => (map.has(key) ? map.get(key) : null),
    setItem: (key, value) => void map.set(key, String(value)),
  };
}

const refusing = {
  getItem() {
    throw new Error("storage is blocked");
  },
  setItem() {
    throw new Error("storage is blocked");
  },
};

afterEach(() => {
  if (original) Object.defineProperty(globalThis, "localStorage", original);
  else delete globalThis.localStorage;
});

test("the key is the one the browser check reads", () => {
  assert.equal(SEEN_KEY, "tetris-lab:tour-seen");
});

test("a fresh browser has not seen the tour; marking it makes it seen", () => {
  const storage = memory();
  install(storage);
  assert.equal(tourSeen(), false);
  markSeen();
  assert.equal(storage.map.get(SEEN_KEY), "1");
  assert.equal(tourSeen(), true);
});

test("only the stored flag counts as seen", () => {
  const storage = memory();
  install(storage);
  storage.map.set(SEEN_KEY, "0");
  assert.equal(tourSeen(), false);
  storage.map.set(SEEN_KEY, "true");
  assert.equal(tourSeen(), false);
  storage.map.set("some-other-key", "1");
  storage.map.delete(SEEN_KEY);
  assert.equal(tourSeen(), false);
});

test("a storage that throws reads as not seen and does not throw on write", () => {
  install(refusing);
  assert.equal(tourSeen(), false);
  assert.doesNotThrow(() => markSeen());
  assert.equal(tourSeen(), false);
});

test("no storage at all reads as not seen and does not throw on write", () => {
  install(undefined);
  assert.equal(tourSeen(), false);
  assert.doesNotThrow(() => markSeen());
});
