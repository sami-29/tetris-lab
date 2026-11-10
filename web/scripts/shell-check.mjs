#!/usr/bin/env node
// The workbench's headless acceptance pass: §2's five regions, at six sizes, in
// three states, with the structural half of §2.1 asserted and the rest left to the eye.
//
//   node scripts/shell-check.mjs                  # serves dist/, six sizes
//   node scripts/shell-check.mjs --shot /tmp/x   # also writes a screenshot per size
//   node scripts/shell-check.mjs --sizes 1440x900
//
// Why a second script rather than more of `browser-check.mjs`: that one drives one
// game at one size and reads the board's pixels at Move k. This one never plays
// anything — it reloads the page at 2560×1440, 1920×1080, 1536×864, 1440×900, 1366×768 and
// 1280×720, in each of
// notes/14 §3's three states (S0 before a Solve, S1 solved, S2 lost), and asks
// whether the *shell* is there. It shares the harness's shape — a static server, the
// DevTools protocol over Node's WebSocket, a `check()` accumulator, a teardown that
// kills Chrome and not Node — and none of its assertions.
//
// The split is the build spec's own ("a test that a tooltip exists would pass on a
// page nobody wants to look at"). A1–A12 below are the mechanical half:
//
//   A1  no page scroll at each size (and `overflow: hidden` on the document)
//   A2  five regions, present, sized and inside the viewport
//   A3  no empty region, in all three states — words *and* a figure or a control
//   A4  every figure labelled in words, with a provenance from the eight words
//   A5  every control has a tooltip
//   A6  the board is drawn, and its geometry contract survives
//   A7  numerals are tabular
//   A8  no emoji, no banned words
//   A9  the keyboard map works, including the guard against a focused field
//   A10 the drawer opens without introducing a page scroll
//   A11 the status bar names Evaluations and shows at least two keyboard chips
//       (Lines and the elapsed time are in the toolbar's outcome line)
//   A12 the figures' sources are honest: `measured` is the clock and nothing else,
//       and no figure is a bare dash
//   A13 a first visit opens the tour, Skip closes it and is remembered
//   A14 the four inner scrollers (readout, layer table, drawer body, Measure body)
//       are never scrolled — Watch, the open drawer and Measure, at every size
//
// What is *not* here, and is judged by eye (notes/14 §8.2): whether 2560×1440 looks
// deliberate, whether each tooltip is true and useful, whether the six-step type
// scale reads as one instrument, whether the icons are one family, and whether the
// band is legible at 24 states per Layer.

import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { extname, join, resolve } from "node:path";

const HERE = new URL(".", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
const WEB = resolve(HERE, "..");
const DIST = resolve(WEB, "dist");

const args = process.argv.slice(2);
const option = (name, fallback) => {
  const at = args.indexOf(`--${name}`);
  return at === -1 ? fallback : args[at + 1];
};

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".wasm": "application/wasm",
  ".map": "application/json",
  ".json": "application/json",
  ".png": "image/png",
  ".svg": "image/svg+xml",
};

const CHROME_CANDIDATES = [
  process.env.CHROME_PATH,
  "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
  "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
  "C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe",
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  "/usr/bin/google-chrome",
  "/usr/bin/chromium",
].filter(Boolean);

/** The target, the laptops, and the design floor (notes/14 §1.5); the 864, 768 and 720 rows are the common laptop heights. */
const SIZES = (option("sizes", "2560x1440,1920x1080,1536x864,1440x900,1366x768,1280x720") ?? "")
  .split(",")
  .map((pair) => pair.split("x").map(Number))
  .filter(([w, h]) => w > 0 && h > 0);

const checks = [];
function check(id, name, ok, detail = "") {
  checks.push({ id, name, ok, detail });
  console.log(`  ${ok ? "✔" : "✘"} ${id} ${name}${detail ? ` — ${detail}` : ""}`);
}

const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

function serve(root) {
  const server = createServer((request, response) => {
    const path = new URL(request.url ?? "/", "http://127.0.0.1").pathname.replace(/\/{2,}/g, "/");
    const file = join(root, decodeURIComponent(path === "/" ? "/index.html" : path));
    if (!file.startsWith(root) || !existsSync(file)) {
      response.writeHead(404).end("not found");
      return;
    }
    const body = readFileSync(file);
    response.writeHead(200, { "content-type": MIME[extname(file)] ?? "application/octet-stream", "content-length": body.length });
    response.end(body);
  });
  return new Promise((done) => server.listen(0, "127.0.0.1", () => done(server)));
}

function connect(url) {
  const ws = new WebSocket(url);
  const pending = new Map();
  let nextId = 0;
  ws.addEventListener("message", (event) => {
    let message;
    try {
      message = JSON.parse(event.data);
    } catch {
      return;
    }
    const entry = pending.get(message.id);
    if (!entry) return;
    pending.delete(message.id);
    if (message.error) entry.reject(new Error(`${message.error.message} (${message.error.code})`));
    else entry.resolve(message.result);
  });
  const ready = new Promise((done, fail) => {
    ws.addEventListener("open", done, { once: true });
    ws.addEventListener("error", () => fail(new Error("the devtools websocket failed")), { once: true });
  });
  const send = (method, params = {}, sessionId) =>
    new Promise((done, fail) => {
      const id = ++nextId;
      pending.set(id, { resolve: done, reject: fail });
      ws.send(JSON.stringify(sessionId ? { id, method, params, sessionId } : { id, method, params }));
    });
  return { ws, ready, send };
}

/**
 * The four inner scrollers, wherever they are showing. A scroller that has anything to
 * scroll is a scrollbar a visitor can reach, and the owner's brief is none: each of
 * these must hold its content at every size. The drawer's body only counts while the
 * drawer is open, and the Measure body only while Measure is.
 */
const SCROLLERS = ["#readout", ".tablewrap", ".dbody", ".measure-body"];
const SCROLL_PROBE = `(() => {
  const out = [];
  for (const selector of ${JSON.stringify(SCROLLERS)}) {
    for (const el of document.querySelectorAll(selector)) {
      if (el.getClientRects().length === 0 || el.clientHeight === 0) continue;
      if (el.closest("#drawer") && document.getElementById("drawer").dataset.open !== "true") continue;
      out.push({ selector, over: el.scrollHeight - el.clientHeight, overX: el.scrollWidth - el.clientWidth, clientHeight: el.clientHeight });
    }
  }
  return out;
})()`;

/**
 * Everything the shell says about itself, in one evaluation.
 *
 * The one rule this expression keeps: it reads *only* what the page publishes —
 * `data-region`, `data-figure`, `data-figure-label`, `data-figure-source`, `data-tip`
 * — so the assertions cannot pass by knowing a private variable.
 */
const READ = `(() => {
  const regionNames = ["toolbar", "rail", "centre", "inspector", "status"];
  const box = (el) => {
    const rect = el.getBoundingClientRect();
    return { x: rect.left, y: rect.top, w: rect.width, h: rect.height, bottom: rect.bottom, right: rect.right };
  };
  const regions = regionNames.map((name) => {
    const el = document.querySelector('[data-region="' + name + '"]');
    if (!el) return { name, present: false };
    return {
      name,
      present: true,
      ...box(el),
      words: (el.innerText || "").replace(/\\s+/g, " ").trim().length,
      figures: el.querySelectorAll("[data-figure]").length,
      controls: el.querySelectorAll("button, input, select, a[href], [role=button], [role=slider]").length,
    };
  });
  const figures = [...document.querySelectorAll("[data-figure]")].map((el) => ({
    id: el.dataset.figure,
    label: (el.dataset.figureLabel || "").trim(),
    source: el.dataset.figureSource || "",
    kind: el.dataset.figureKind || "",
    text: (el.innerText || "").replace(/\\s+/g, " ").trim(),
  }));
  const controls = [...document.querySelectorAll("[data-region] button, [data-region] input, [data-region] select, [data-region] a[href], [data-region] [role=button], [data-region] [role=slider]")];
  const canvas = document.getElementById("board");
  const ctx = canvas ? canvas.getContext("2d") : null;
  let lit = 0;
  let painted = 0;
  if (ctx && canvas.width > 0 && canvas.height > 0) {
    const data = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
    for (let i = 0; i < data.length; i += 4) {
      if (data[i] > 90 && data[i + 1] > 110 && data[i + 2] > 140) lit += 1;
      // Not the well's own background (#0b0f14): grid, stack, piece, contact edge.
      if (Math.abs(data[i] - 11) > 8 || Math.abs(data[i + 1] - 15) > 8 || Math.abs(data[i + 2] - 20) > 8) painted += 1;
    }
  }
  const statusBar = document.getElementById("statusbar");
  const drawer = document.getElementById("drawer");
  const sheet = document.getElementById("sheet");
  return {
    viewport: { w: window.innerWidth, h: window.innerHeight },
    overflow: getComputedStyle(document.documentElement).overflow,
    bodyOverflow: getComputedStyle(document.body).overflow,
    scrollHeight: document.scrollingElement.scrollHeight,
    bodyScrollHeight: document.body.scrollHeight,
    regions,
    figures,
    controlCount: controls.length,
    tippedControls: controls.filter((el) => (el.dataset.tip || el.getAttribute("data-tip") || "").trim().length > 0).length,
    figuresWithoutLabel: document.querySelectorAll("[data-figure]:not([data-figure-label])").length,
    numFont: (() => {
      const num = document.querySelector("[data-figure] .num");
      return num ? getComputedStyle(num).fontVariantNumeric || getComputedStyle(num).fontFeatureSettings : "";
    })(),
    canvas: {
      width: canvas?.width ?? 0,
      height: canvas?.height ?? 0,
      cell: canvas?.dataset.cell ?? "",
      originX: canvas?.dataset.originX ?? "",
      originY: canvas?.dataset.originY ?? "",
      rows: canvas?.dataset.rows ?? "",
      cols: canvas?.dataset.cols ?? "",
      lit,
      painted,
    },
    statusWords: (statusBar?.innerText || "").replace(/\\s+/g, " "),
    kbdChips: statusBar ? statusBar.querySelectorAll(".kbd").length : 0,
    outcome: (document.getElementById("outcome")?.textContent || "").trim(),
    boot: (document.getElementById("boot")?.textContent || "").trim(),
    drawerOpen: drawer?.dataset.open === "true",
    sheetOpen: sheet?.dataset.open === "true",
    text: (document.body.innerText || "").slice(0, 4000),
    playing: document.getElementById("play")?.getAttribute("aria-pressed") === "true",
    seek: Number(document.getElementById("strip")?.getAttribute("aria-valuenow") ?? -1),
    gridTracks: (() => {
      const app = document.getElementById("app");
      return app ? getComputedStyle(app).gridTemplateColumns : "";
    })(),
    gridRows: (() => {
      const app = document.getElementById("app");
      return app ? getComputedStyle(app).gridTemplateRows : "";
    })(),
  };
})()`;

let exitCode = 1;
let server = null;
let chrome = null;
let profile = null;

try {
  if (!existsSync(join(DIST, "index.html"))) throw new Error(`no ${DIST}/index.html — run \`npm run build\` first`);
  server = await serve(DIST);
  const target = `http://127.0.0.1:${server.address().port}/`;

  const chromePath = CHROME_CANDIDATES.find((candidate) => existsSync(candidate));
  if (!chromePath) throw new Error("no Chrome found; set CHROME_PATH");

  const port = 9400 + Math.floor(Math.random() * 300);
  profile = mkdtempSync(join(tmpdir(), "tetris-shell-"));
  chrome = spawn(
    chromePath,
    [
      "--headless=new",
      "--disable-gpu",
      "--disable-background-timer-throttling",
      "--disable-backgrounding-occluded-windows",
      "--disable-renderer-backgrounding",
      "--no-first-run",
      "--no-default-browser-check",
      `--remote-debugging-port=${port}`,
      `--user-data-dir=${profile}`,
      "about:blank",
    ],
    { stdio: "ignore" },
  );

  let debuggerUrl = null;
  for (let i = 0; i < 150 && !debuggerUrl; i += 1) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json/version`);
      if (response.ok) debuggerUrl = (await response.json()).webSocketDebuggerUrl;
    } catch {
      // not up yet
    }
    if (!debuggerUrl) await sleep(100);
  }
  if (!debuggerUrl) throw new Error("the browser's devtools endpoint never came up");

  const cdp = connect(debuggerUrl);
  await cdp.ready;
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  await cdp.send("Target.activateTarget", { targetId });
  await cdp.send("Page.enable", {}, sessionId);
  await cdp.send("Runtime.enable", {}, sessionId);
  // A first visit opens the tour. Every check but A13 arrives as a returning visitor
  // (the key is `SEEN_KEY` in `src/ui/tour.ts`); A13 drops this script to be new.
  const SEEN = `try { localStorage.setItem("tetris-lab:tour-seen", "1"); } catch {}`;
  const returning = await cdp.send("Page.addScriptToEvaluateOnNewDocument", { source: SEEN }, sessionId);

  const read = async () => {
    const result = await cdp.send("Runtime.evaluate", { expression: READ, returnByValue: true }, sessionId);
    return result.result?.value ?? null;
  };

  /** Reload at `size`, optionally with the worker stubbed out (the S0 state). */
  const load = async (size, url, { stubWorker = false } = {}) => {
    await cdp.send("Emulation.setDeviceMetricsOverride", { width: size[0], height: size[1], deviceScaleFactor: size[2] ?? 1, mobile: false }, sessionId);
    let stub = null;
    if (stubWorker) {
      // S0 is "before any Solve has returned". Waiting for it is a race; removing the
      // worker makes the state stable, and it is the state a visitor on a slow link
      // sees, so it has to be checked.
      stub = await cdp.send(
        "Page.addScriptToEvaluateOnNewDocument",
        { source: "window.Worker = class { constructor() {} postMessage() {} terminate() {} addEventListener() {} };" },
        sessionId,
      );
    }
    await cdp.send("Page.navigate", { url: new URL(url.replace(/^\//, ""), target).href }, sessionId);
    for (let i = 0; i < 60; i += 1) {
      await sleep(100);
      const state = await read();
      if (state && state.regions.length === 5) break;
    }
    if (stub) await cdp.send("Page.removeScriptToEvaluateOnNewDocument", { identifier: stub.identifier }, sessionId);
    await sleep(120);
    return read();
  };

  /** Wait until the toolbar names an answer rather than "Solving seed …". */
  const solved = async () => {
    for (let i = 0; i < 80; i += 1) {
      const state = await read();
      if (state && state.outcome.length > 0 && !state.outcome.startsWith("Solving ")) return state;
      await sleep(100);
    }
    return read();
  };

  const shoot = async (label) => {
    const shot = option("shot", null);
    if (!shot) return;
    const capture = await cdp.send("Page.captureScreenshot", { format: "png" }, sessionId);
    const base = shot.endsWith(".png") ? shot.slice(0, -4) : shot;
    const file = `${base}-${label}.png`;
    writeFileSync(file, Buffer.from(capture.data, "base64"));
    console.log(`  screenshot ${file}`);
  };

  const dispatch = async (key, code, vk) => {
    await cdp.send("Input.dispatchKeyEvent", { type: "rawKeyDown", key, code, windowsVirtualKeyCode: vk }, sessionId);
    await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", key, code, windowsVirtualKeyCode: vk }, sessionId);
    await sleep(160);
  };

  const EXPECTED_SOURCES = ["solve", "trace", "replay", "dataset", "reference", "url", "measured", "control"];
  const banned = /[\u{1F300}-\u{1FAFF}\u{2600}-\u{27BF}\u{2B00}-\u{2BFF}]/u;

  /** A1, A2, A3, A11 — every state, every size. */
  const regionChecks = (state, label) => {
    const { viewport } = state;
    check("A1", `no page scroll · ${label}`, state.scrollHeight <= viewport.h && state.bodyScrollHeight <= viewport.h && state.overflow === "hidden" && state.bodyOverflow === "hidden",
      `scrollHeight ${state.scrollHeight} / body ${state.bodyScrollHeight} / viewport ${viewport.h} / overflow ${state.overflow}`);
    const missing = state.regions.filter((region) => !region.present || region.w <= 0 || region.h <= 0);
    const escaped = state.regions.filter((region) => region.present && (region.y < -0.5 || region.bottom > viewport.h + 0.5));
    check("A2", `five regions, in the viewport · ${label}`, missing.length === 0 && escaped.length === 0,
      [missing.map((region) => `${region.name} missing`).join(", "), escaped.map((region) => `${region.name} ${Math.round(region.bottom)}>${viewport.h}`).join(", ")].filter(Boolean).join(" · ") || state.regions.map((region) => `${region.name} ${Math.round(region.w)}×${Math.round(region.h)}`).join(" · "));
    const empty = state.regions.filter((region) => region.present && (region.words <= 20 || region.figures + region.controls === 0));
    check("A3", `no empty region · ${label}`, empty.length === 0,
      empty.map((region) => `${region.name}: ${region.words} chars, ${region.figures} figures`).join("; ") || state.regions.map((region) => `${region.name} ${region.words}c/${region.figures}f`).join(" · "));
    const words = state.statusWords;
    // The outcome line carries Lines and the elapsed time; the status bar keeps the
    // one solve number shown nowhere else, beside the keyboard map.
    check("A11", `the status bar names Evaluations beside the keyboard map · ${label}`, words.includes("Evaluations") && state.kbdChips >= 2,
      `${words.includes("Evaluations") ? "Evaluations" : "no Evaluations"} · ${state.kbdChips} chips`);
  };

  /** A4–A8, A12: the figure and tooltip lints, once per state. */
  const figureChecks = (state, label, { drawn = true } = {}) => {
    const bad = state.figures.filter((figure) => figure.label.length <= 2 || !EXPECTED_SOURCES.includes(figure.source));
    check("A4", `every figure labelled with its provenance · ${label}`, bad.length === 0 && state.figuresWithoutLabel === 0,
      `${state.figures.length} figures${bad.length > 0 ? ` · ${bad.map((figure) => `${figure.id}:${figure.label || "?"}/${figure.source}`).join(", ")}` : ""}`);
    check("A5", `every control has a tooltip · ${label}`, state.tippedControls === state.controlCount,
      `${state.tippedControls}/${state.controlCount} controls`);
    check(
      "A6",
      `the board is drawn and its geometry is readable · ${label}`,
      state.canvas.painted > 1000 && state.canvas.cell !== "" && state.canvas.originX !== "" && state.canvas.rows === "20" && state.canvas.cols === "10" && (!drawn || state.canvas.lit > 0),
      `${state.canvas.lit} lit / ${state.canvas.painted} painted pixels · cell ${state.canvas.cell} · origin ${state.canvas.originX},${state.canvas.originY} · ${state.canvas.cols}×${state.canvas.rows}`,
    );
    check("A7", `numerals are tabular · ${label}`, /tabular-nums|tnum/.test(state.numFont), state.numFont);
    const glyph = banned.exec(state.text);
    const bannedWords = /numpy|adaptive beam|Flask|requirements\.txt/i.exec(state.text);
    let why = "clean";
    if (glyph) why = `glyph ${JSON.stringify(glyph[0])}`;
    else if (bannedWords) why = `word ${bannedWords[0]}`;
    check("A8", `no emoji, no banned words · ${label}`, glyph === null && bannedWords === null, why);
    // A12: `measured` is the clock and nothing else, and no figure is a bare dash.
    // A12, in the notes' words: "no `measured` figure outside {elapsed, speed}". A
    // Run's games-done and the log's stamps are also `performance.now()` values, and
    // they are the clock by definition, so the rule is "the clock and nothing that
    // claims to be the engine".
    const measured = state.figures.filter((figure) => figure.source === "measured" && !/elapsed|speed|progress|log/i.test(figure.label));
    const dashes = state.figures.filter((figure) => /^[\s\-\u2014]*$/.test(figure.text) && figure.kind === "number");
    let honesty = `${state.figures.filter((figure) => figure.source === "measured").length} measured (elapsed only)`;
    if (measured.length > 0) honesty = `measured: ${measured.map((figure) => figure.id).join(",")}`;
    else if (dashes.length > 0) honesty = `dash: ${dashes.map((figure) => figure.id).join(",")}`;
    check("A12", `the figures' sources are honest · ${label}`, measured.length === 0 && dashes.length === 0, honesty);
  };

  const layoutChecks = (state, size) => {
    // The stylesheet's arithmetic, read back from the page: the rail is 236 px (212
    // at 1280 and under), the centre is as wide as the board it holds, the inspector
    // takes the rest with a 400 px floor, and the rows are 48 / 1fr / 28.
    const [width, height] = size;
    const rail = width <= 1280 ? 212 : 236;
    const byName = Object.fromEntries(state.regions.map((region) => [region.name, region]));
    const expectInspector = width - rail - byName.centre.w;
    const okInspector = byName.inspector.w >= 399 && Math.abs(byName.inspector.w - expectInspector) <= 2;
    const okToolbar = Math.abs(byName.toolbar.h - 48) <= 1;
    const okStatus = Math.abs(byName.status.h - 28) <= 1;
    const okRail = Math.abs(byName.rail.w - rail) <= 1;
    const stage = height - 48 - 28;
    const okStage = Math.abs(byName.centre.h - stage) <= 1;
    check(
      "A2b",
      `the grid is the stylesheet's arithmetic · ${width}×${height}`,
      okInspector && okToolbar && okStatus && okRail && okStage,
      `rail ${Math.round(byName.rail.w)}/${rail} · inspector ${Math.round(byName.inspector.w)}/${Math.round(expectInspector)} (≥400) · toolbar ${Math.round(byName.toolbar.h)}/48 · status ${Math.round(byName.status.h)}/28 · stage ${Math.round(byName.centre.h)}/${stage}`,
    );
  };

  console.log(`shell check · ${target}`);
  console.log(`sizes ${SIZES.map(([w, h]) => `${w}×${h}`).join(", ")}`);

  for (const size of SIZES) {
    const [width, height] = size;
    const label = `${width}×${height}${size[2] ? `@${size[2]}x` : ""}`;
    console.log(`\n── ${label} ──`);

    // S0: before any Solve has returned. The worker is stubbed out, so this is a
    // stable state rather than a race, and it is the state that decides whether the
    // first paint has content (§3's corollary).
    const s0 = await load(size, "/", { stubWorker: true });
    regionChecks(s0, `S0 · ${label}`);
    // S0 has an empty well on purpose — the grid is drawn, the cells are not — so
    // A6 asks for a painted well and the geometry contract, not for filled cells.
    figureChecks(s0, `S0 · ${label}`, { drawn: false });
    layoutChecks(s0, size);
    check("A3b", `S0 says it is solving, not that it is broken · ${label}`, /^Solving seed \d+…$/.test(s0.outcome) && s0.canvas.lit >= 0,
      s0.outcome.slice(0, 60));
    await shoot(`s0-${width}x${height}`);

    // S1: solved, with the replay on Move 0.
    await load(size, "/");
    const s1 = await solved();
    regionChecks(s1, `S1 · ${label}`);
    figureChecks(s1, `S1 · ${label}`);
    layoutChecks(s1, size);
    check("S1b", `the solved outcome is the toolbar's · ${label}`, /^Win · \d+ of \d+ lines · [\d.]+ ms$/.test(s1.outcome), s1.outcome);
    await shoot(`s1-${width}x${height}`);

    // A14: the inner scrollers hold their content — Watch, then the drawer, then Measure.
    const probe = async (view) => {
      const found = (await cdp.send("Runtime.evaluate", { expression: SCROLL_PROBE, returnByValue: true }, sessionId)).result?.value ?? [];
      const spilled = found.filter((entry) => entry.over > 0 || entry.overX > 0);
      check("A14", `no inner scroller scrolls · ${view} · ${label}`, found.length > 0 && spilled.length === 0,
        spilled.map((entry) => `${entry.selector} +${entry.over}px tall${entry.overX > 0 ? ` +${entry.overX}px wide` : ""} (of ${entry.clientHeight})`).join("; ")
          || `${found.map((entry) => entry.selector).join(", ")} fit`);
    };
    const press = (expression) => cdp.send("Runtime.evaluate", { expression, returnByValue: true }, sessionId);
    await probe("Watch");
    await press(`document.getElementById("view-layers").click()`);
    await sleep(250);
    await probe("Watch + Layers");
    await press(`document.getElementById("view-beam").click()`);
    await dispatch("e", "KeyE", 69);
    await probe("Watch + drawer");
    await dispatch("Escape", "Escape", 27);
    await press(`document.getElementById("mode-measure").click()`);
    await sleep(250);
    await probe("Measure");
    // The same page with a Run's result on it: thirty seeds, a second or so.
    await press(`(document.getElementById("run-count").value = "30", document.getElementById("run-start").click())`);
    for (let i = 0; i < 100; i += 1) {
      await sleep(200);
      const status = (await press(`document.getElementById("run-state").textContent`)).result?.value ?? "";
      if (/^(\d+)\/\1 games/.test(status)) break;
    }
    await probe("Measure + a Run");
    await shoot(`measure-${width}x${height}`);

    // S2: a lost game — seed 7 at goal 20 is a game the shipped Evaluator loses
    // (established by `browser-check.mjs`'s "a game the search cannot win" check).
    await load(size, "/?seed=7&goal=20");
    const s2 = await solved();
    regionChecks(s2, `S2 · ${label}`);
    figureChecks(s2, `S2 · ${label}`);
    check("S2b", `a loss is an outcome and says the Lines it reached · ${label}`, /^Loss · reached \d+ of 20 lines · [\d.]+ ms$/.test(s2.outcome), s2.outcome);
    check("S2c", `a loss still fills every region · ${label}`, s2.regions.every((region) => region.words > 20 && region.figures + region.controls > 0),
      s2.regions.map((region) => `${region.name} ${region.words}c`).join(" · "));
    await shoot(`s2-${width}x${height}`);
  }

  // ── A9, A10: the keyboard map and the drawer, once, at the design floor ──────
  console.log("\n── keyboard and drawer · 1440×900 ──");
  const floor = SIZES.find(([w, h]) => w === 1440 && h === 900) ?? SIZES[SIZES.length - 1];
  await load(floor, "/");
  const solvedState = await solved();
  check("A9a", "the page starts paused on Move 0", solvedState.seek === 0 && !solvedState.playing, `Move ${solvedState.seek} · ${solvedState.playing ? "playing" : "paused"}`);

  await dispatch("ArrowRight", "ArrowRight", 39);
  const stepped = await read();
  check("A9b", "→ steps one Move", stepped.seek === 1, `Move ${stepped.seek}`);

  await dispatch("e", "KeyE", 69);
  const drawerOpen = await read();
  check("A9c", "E opens the Explainer drawer", drawerOpen.drawerOpen, `drawer ${drawerOpen.drawerOpen}`);
  check("A10", "the drawer introduces no page scroll", drawerOpen.scrollHeight <= drawerOpen.viewport.h, `${drawerOpen.scrollHeight} / ${drawerOpen.viewport.h}`);
  await shoot("drawer-1440x900");

  await dispatch("Escape", "Escape", 27);
  check("A9d", "Escape closes it", !(await read()).drawerOpen);

  await dispatch("?", "Slash", 191);
  check("A9e", "? opens the keyboard sheet inside the drawer", (await read()).sheetOpen);
  await dispatch("Escape", "Escape", 27);
  check("A9f", "Escape closes the sheet and the drawer with it", !(await read()).sheetOpen && !(await read()).drawerOpen);

  // The guard: a focused number field keeps its own keys, so typing a space in the
  // seed field is a space and not a play/pause. Paused first, because the page starts
  // playing 400 ms after a Solve (ticket 10's "show the starting board" beat) and a
  // running clock would move the playhead between the two reads whatever the key did.
  await dispatch(" ", "Space", 32);
  const paused = await read();
  const beforeSpace = await read();
  await cdp.send("Runtime.evaluate", { expression: `document.getElementById("pg-seed").focus()`, returnByValue: true }, sessionId);
  const focused = await cdp.send("Runtime.evaluate", { expression: `document.activeElement.id`, returnByValue: true }, sessionId);
  await dispatch(" ", "Space", 32);
  await sleep(400);
  const afterSpace = await read();
  check(
    "A9g",
    "Space in a focused field is not play/pause",
    focused.result?.value === "pg-seed" && afterSpace.seek === beforeSpace.seek && afterSpace.seek === paused.seek,
    `focus ${focused.result?.value} · Move ${beforeSpace.seek} → ${afterSpace.seek} · ${afterSpace.playing ? "playing" : "paused"}`,
  );
  await cdp.send("Runtime.evaluate", { expression: `document.getElementById("pg-seed").blur()`, returnByValue: true }, sessionId);
  await dispatch(" ", "Space", 32);
  await sleep(700);
  const played = await read();
  check("A9h", "Space plays from a blurred page", played.seek >= 1 && played.playing, `Move ${played.seek} · ${played.playing ? "playing" : "paused"}`);

  // ── A13: a first visit gets the tour; Skip is remembered ─────────────────────
  console.log("\n── first visit · 1440×900 ──");
  const tourOpen = `!document.getElementById("tour")?.hidden`;
  const evaluate = async (expression) =>
    (await cdp.send("Runtime.evaluate", { expression, returnByValue: true }, sessionId)).result?.value;
  await cdp.send("Page.removeScriptToEvaluateOnNewDocument", { identifier: returning.identifier }, sessionId);
  await evaluate(`localStorage.removeItem("tetris-lab:tour-seen")`);
  await load(floor, "/");
  await sleep(300);
  check("A13a", "a first visit opens the tour", await evaluate(tourOpen) === true);
  await evaluate(`document.getElementById("tour-skip").click()`);
  check("A13b", "Skip closes it and remembers", await evaluate(`${tourOpen} === false && localStorage.getItem("tetris-lab:tour-seen") === "1"`) === true);
  await load(floor, "/");
  await sleep(300);
  check("A13c", "a returning visitor is not shown it again", await evaluate(tourOpen) === false);

  const failed = checks.filter((entry) => !entry.ok);
  exitCode = failed.length === 0 ? 0 : 1;
  console.log(
    exitCode === 0
      ? `\nPASS the shell is five regions at ${SIZES.length} sizes, in three states, with ${checks.length} assertions`
      : `\nFAIL ${failed.length} of ${checks.length} check(s): ${failed.map((entry) => `${entry.id} ${entry.name}`).join("; ")}`,
  );
} catch (error) {
  console.error(`shell check failed: ${error.message}`);
  exitCode = 1;
} finally {
  if (chrome) chrome.kill();
  if (server) server.close();
  await sleep(300);
  if (profile) {
    try {
      rmSync(profile, { recursive: true, force: true });
    } catch {
      // Windows may hold a handle on the profile directory; it is disposable.
    }
  }
}
process.exit(exitCode);
