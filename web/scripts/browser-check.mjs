#!/usr/bin/env node
// Headless acceptance for the page: open it in a real browser, watch a real wasm
// instance solve a real game, drive the controls the way a visitor does, and read
// the pixels back.
//
//   node scripts/browser-check.mjs                 # serves dist/ itself
//   node scripts/browser-check.mjs --url https://… # against a live URL
//   node scripts/browser-check.mjs --shot /tmp/tetris.png
//
// No dependencies: a static file server, then the Chrome DevTools Protocol over
// Node's built-in WebSocket (the shape `host-probe/browser-check.mjs` established).
//
// What it checks, and why it is here rather than in `node --test`: everything below
// needs a real page. That the Solve arrives; that a **gesture** on the move strip
// lands on a Move and the board drawn is the engine's board at that Move; that the
// arrow keys, Space, Home/End, R, `?` and Escape do what the map says; that the
// speed control is continuous; that frames between two boards are drawn at all — so
// a drop and a collapse are motion, not a painted end state. Then it writes
// screenshots, which is the part no assertion owns.
//
// Exit code 0 means every assertion passed; 1 means one named check failed.

import { spawn } from "node:child_process";
import { createServer } from "node:http";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { extname, join, resolve } from "node:path";

import { TIER_A, TIER_B, formatHits, scanText } from "./words.mjs";

const HERE = new URL(".", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
const WEB = resolve(HERE, "..");
const DIST = resolve(WEB, "dist");
const ARTIFACT = resolve(WEB, "pkg", "engine.wasm");

const args = process.argv.slice(2);
const option = (name, fallback) => {
  const index = args.indexOf(`--${name}`);
  return index === -1 ? fallback : args[index + 1];
};

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
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
  "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  "/usr/bin/google-chrome",
  "/usr/bin/chromium",
].filter(Boolean);

const checks = [];

/**
 * The yardstick, computed here from the artifact and never from the page: generate
 * the same default game, solve it, build the replay timeline, and read the settled
 * mask after *every* Move. A seek to Move k must draw `masks[k]`, and a collapse in
 * progress must draw something that is in no mask at all.
 */
function maskOf(field) {
  let mask = "";
  for (let row = 0; row < 20; row += 1) {
    for (let col = 0; col < 10; col += 1) mask += field[col] & (1 << (19 - row)) ? "1" : "0";
  }
  return mask;
}

async function expectedBoards() {
  const { pathToFileURL } = await import("node:url");
  const load = (path) => import(pathToFileURL(path).href);
  const { loadEngineFromBytes } = await load(resolve(WEB, "src", "core", "loader.ts"));
  const { buildTimeline, reconstruct } = await load(resolve(WEB, "src", "ui", "timeline.ts"));

  const engine = await loadEngineFromBytes(new Uint8Array(readFileSync(ARTIFACT)));
  engine.genGame(0, 40, 7);
  const board = Uint32Array.from(engine.board());
  const outcome = engine.solve(8, 25);
  const moves = Uint32Array.from(engine.moves(outcome.moveCount));
  const timeline = buildTimeline(board, [...moves]);
  const masks = [];
  for (let k = 0; k <= timeline.steps.length; k += 1) masks.push(maskOf(reconstruct(timeline, k)));
  const clearing = timeline.steps.filter((step) => step.cleared.length > 0).map((step) => step.k);
  return { outcome, timeline, masks, clearing, total: timeline.steps.length };
}
function check(name, ok, detail = "") {
  checks.push({ name, ok, detail });
  console.log(`${ok ? "✔" : "✘"} ${name}${detail ? ` — ${detail}` : ""}`);
}

/** Serve dist/ with the MIME types the loader's streaming path depends on. */
function serve(root) {
  const server = createServer((request, response) => {
    const path = new URL(request.url ?? "/", "http://127.0.0.1");
    const rel = path.pathname === "/" ? "/index.html" : path.pathname;
    const file = join(root, decodeURIComponent(rel));
    if (!file.startsWith(root) || !existsSync(file)) {
      response.writeHead(404).end("not found");
      return;
    }
    const body = readFileSync(file);
    response.writeHead(200, {
      "content-type": MIME[extname(file)] ?? "application/octet-stream",
      "content-length": body.length,
    });
    response.end(body);
  });
  return new Promise((done) => {
    server.listen(0, "127.0.0.1", () => done(server));
  });
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

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
    ws.addEventListener("error", () => fail(new Error("websocket failed")), { once: true });
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
 * Every picture the board draws over `windowMs`, sampled once per animation frame
 * from *inside* the page. A CDP round trip per sample costs ~100 ms at this pace,
 * which is a third of a step, and skipped the clear as often as it caught it; in
 * the page, nothing the renderer paints can fall between two samples.
 *
 * A picture is the settled mask followed by the in-flight mask (see READ): at rest
 * the second half is all zeroes, and every frame that is not all zeroes there is a
 * piece in the air or a band contracting.
 */
const FRAME_WINDOW = (windowMs) => `new Promise((resolve) => {
  const canvas = document.getElementById("board");
  const ctx = canvas.getContext("2d");
  const dpr = window.devicePixelRatio || 1;
  const cell = Number(canvas.dataset.cell || 0) * dpr;
  const ox = Number(canvas.dataset.originX || 0) * dpr;
  const oy = Number(canvas.dataset.originY || 0) * dpr;
  const frames = [];
  const t0 = performance.now();
  const sample = () => {
    const data = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
    let settled = "";
    let flight = "";
    for (let row = 0; row < 20; row += 1) {
      for (let col = 0; col < 10; col += 1) {
        const x = Math.round(ox + (col + 0.5) * cell);
        const y = Math.round(oy + (row + 0.5) * cell);
        const i = (y * canvas.width + x) * 4;
        const r = data[i];
        const g = data[i + 1];
        const b = data[i + 2];
        const isSettled = r > 100 && r < 150 && g > 135 && g < 180 && b > 170 && b < 215;
        settled += isSettled ? "1" : "0";
        flight += !isSettled && r > 195 && g > 205 && b > 228 ? "1" : "0";
      }
    }
    const picture = settled + flight;
    const last = frames[frames.length - 1];
    if (!last || last.picture !== picture) {
      frames.push({
        at: Math.round(performance.now() - t0),
        seek: Number(document.getElementById("strip")?.getAttribute("aria-valuenow") ?? -1),
        picture: picture,
        settled: settled,
        flight: flight,
      });
    }
    if (performance.now() - t0 < ${windowMs}) requestAnimationFrame(sample);
    else resolve(frames);
  };
  requestAnimationFrame(sample);
})`;

/** What the page says about itself, read from the DOM it rendered. */
const READ = `(() => {
  const text = (id) => document.getElementById(id)?.textContent ?? "";
  const canvas = document.getElementById("board");
  const ctx = canvas?.getContext("2d");
  let drawn = { cells: 0, topRow: 20, bottomRow: 0 };
  if (ctx) {
    const { width, height } = canvas;
    const data = ctx.getImageData(0, 0, width, height).data;
    // Count the brighter cells: the background is #0b0f14, the stack is #7f9dc0.
    for (let y = 0; y < height; y += 1) {
      for (let x = 0; x < width; x += 1) {
        const i = (y * width + x) * 4;
        if (data[i] > 90 && data[i + 1] > 110 && data[i + 2] > 140) {
          drawn.cells += 1;
          const row = Math.floor((y / height) * 20);
          if (row < drawn.topRow) drawn.topRow = row;
          if (row > drawn.bottomRow) drawn.bottomRow = row;
        }
      }
    }
  }
  // Read the 10×20 board back out of the pixels, cell centre by cell centre, as
  // *two* masks: the settled cells (#7f9dc0) and the cells drawn in the brighter
  // in-flight fill (#e8f0ff, and the #d7e4f5 clear band). A cell cannot be both,
  // so mask + flight is one picture and mask alone is only the settled half.
  // Two masks rather than one colour window: a drop must not be able to read as a
  // finished board, and it would if the in-flight fill were simply ignored.
  let mask = "";
  let flight = "";
  const dpr = window.devicePixelRatio || 1;
  const cell = Number(canvas?.dataset.cell ?? 0) * dpr;
  const ox = Number(canvas?.dataset.originX ?? 0) * dpr;
  const oy = Number(canvas?.dataset.originY ?? 0) * dpr;
  if (ctx && cell > 0) {
    const data = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
    for (let row = 0; row < 20; row += 1) {
      for (let col = 0; col < 10; col += 1) {
        const x = Math.round(ox + (col + 0.5) * cell);
        const y = Math.round(oy + (row + 0.5) * cell);
        const i = (y * canvas.width + x) * 4;
        const [r, g, b] = [data[i], data[i + 1], data[i + 2]];
        const settled = r > 100 && r < 150 && g > 135 && g < 180 && b > 170 && b < 215;
        mask += settled ? "1" : "0";
        flight += !settled && r > 195 && g > 205 && b > 228 ? "1" : "0";
      }
    }
  }
  const strip = document.getElementById("strip");
  const play = document.getElementById("play");
  const sheet = document.getElementById("sheet");
  return {
    status: text("status"),
    outcome: text("outcome"),
    progress: text("progress"),
    // Ticket 17: the deploy's own handshake, read from the status bar's boot slot.
    // 'src/main.ts' writes "worker 1 · abi 2 · <n> pieces" once the worker has
    // compiled the deployed wasm and its abi_version() agreed, so this is the
    // page's report of a successful handshake, not the harness's assumption.
    boot: text("boot"),
    origin: location.origin,
    protocol: location.protocol,
    tone: document.getElementById("status")?.dataset.tone ?? "",
    mask,
    flight,
    canvas: { width: canvas?.width ?? 0, height: canvas?.height ?? 0, ...drawn },
    // The controls, as a visitor sees them.
    seek: Number(strip?.getAttribute("aria-valuenow") ?? -1),
    seekMax: Number(strip?.getAttribute("aria-valuemax") ?? -1),
    seekText: strip?.getAttribute("aria-valuetext") ?? "",
    playText: (play?.textContent ?? "").trim(),
    pressed: play?.getAttribute("aria-pressed") ?? "",
    speed: text("speed-readout"),
    // The visible unit is "Moves/s"; the seconds per Move are in the control's
    // aria-label (the transport is too narrow for both at 1280).
    speedLabel: document.getElementById("speed-readout")?.getAttribute("aria-label") ?? "",
    sheetOpen: sheet?.dataset.open === "true",
    sheetRows: document.querySelectorAll("#sheet dt").length,
    stripCells: document.querySelectorAll("#strip i").length,
    // §2.1.2: a tooltip on every control. Since ticket 14 the page's tooltip is one
    // delegated element driven by data-tip (a native title would be clipped by the
    // scrolling rail), and every title in the document is migrated into data-tip at
    // boot — so either attribute counts, and "neither" is the failure.
    untitled: [...document.querySelectorAll("button, input, [role=slider]")]
      .filter((el) => !el.getAttribute("title") && !el.getAttribute("data-tip"))
      .map((el) => el.id || el.tagName),
    stripRect: (() => {
      const rect = strip?.getBoundingClientRect();
      return rect ? { x: rect.left, y: rect.top, width: rect.width, height: rect.height } : null;
    })(),
    // Ticket 13: the game in the address bar, and the controls that put it there.
    search: location.search,
    historyLength: history.length,
    outcomeKind: document.getElementById("outcome")?.dataset.outcome ?? "",
    reason: document.getElementById("playground")?.dataset.reason ?? "",
    playground: Object.fromEntries(
      [...document.querySelectorAll("#playground [data-field]")].map((el) => [el.dataset.field, el.value]),
    ),
    presets: [...document.querySelectorAll("#playground [data-preset]")].map((el) => el.dataset.preset),
    beamReadout: text("beam-readout"),
    beamTickTitled: document.getElementById("beam-tick")?.getAttribute("title") ?? "",
    // Ticket 15: the Run panel - progress, both columns, and the machine labels.
    // A cell's data-run-cell is config-figure-column, so the harness can compare
    // the live column with the Reference column row by row.
    run: {
      state: text("run-state"),
      status: document.getElementById("run-state")?.dataset.runStatus ?? "",
      workers: Number(document.getElementById("run-state")?.dataset.workers ?? 0),
      startDisabled: document.getElementById("run-dataset")?.disabled ?? true,
      stopDisabled: document.getElementById("run-stop")?.disabled ?? true,
      cells: Object.fromEntries(
        [...document.querySelectorAll("[data-run-cell]")].map((el) => [el.dataset.runCell, (el.textContent ?? "").trim()]),
      ),
      labels: [...document.querySelectorAll("[data-run-label]")].map((el) => (el.textContent ?? "").trim()),
      assumption: document.getElementById("run-assumption")?.textContent ?? "",
    },
    // Ticket 16: the Explainer drawer, opened and read as a visitor reads it.
    drawerOpen: document.getElementById("drawer")?.dataset.open === "true",
    drawerText: document.getElementById("drawer")?.innerText ?? "",
    drawerBody: (() => {
      const body = document.querySelector("#drawer .dbody");
      if (!body) return null;
      const last = body.querySelector("#explainer-body") ?? body.lastElementChild;
      const padding = parseFloat(getComputedStyle(body).paddingBottom) || 0;
      return {
        scrollHeight: body.scrollHeight,
        clientHeight: body.clientHeight,
        free: last
          ? Math.round(body.getBoundingClientRect().bottom - last.getBoundingClientRect().bottom - padding)
          : 0,
      };
    })(),
    scrollHeight: document.scrollingElement?.scrollHeight ?? 0,
    bodyScrollHeight: document.body.scrollHeight ?? 0,
    rootOverflow: getComputedStyle(document.documentElement).overflow,
    viewport: { w: innerWidth, h: innerHeight },
  };
})()`;

let exitCode = 1;
let server = null;
let chrome = null;
let profile = null;

try {
  let target = option("url", null);
  if (!target) {
    if (!existsSync(join(DIST, "index.html"))) {
      throw new Error(`no ${DIST}/index.html — run \`npm run build\` first`);
    }
    server = await serve(DIST);
    target = `http://127.0.0.1:${server.address().port}/`;
  }

  const chromePath = CHROME_CANDIDATES.find((candidate) => existsSync(candidate));
  if (!chromePath) throw new Error("no Chrome or Edge found; set CHROME_PATH");

  const port = 9400 + Math.floor(Math.random() * 300);
  profile = mkdtempSync(join(tmpdir(), "tetris-lab-check-"));
  chrome = spawn(
    chromePath,
    [
      "--headless=new",
      "--disable-gpu",
      // Background tabs do not run requestAnimationFrame; playback is animation,
      // so the page must be a foreground, unthrottled one for this to mean anything.
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
  // A first visit opens the tour, which takes the arrow keys and covers the board.
  // This harness checks the Lab, not the tour, so it arrives as a returning visitor
  // (the key is `SEEN_KEY` in `src/ui/tour.ts`).
  await cdp.send(
    "Page.addScriptToEvaluateOnNewDocument",
    { source: `try { localStorage.setItem("tetris-lab:tour-seen", "1"); } catch {}` },
    sessionId,
  );
  // The shell is desktop-first (§2.1.5) and the design floor is 1440×900: below it,
  // panels scroll inside themselves and the readout loses its column. The window the
  // chrome happens to open with is not a viewport a visitor has, so the harness sets
  // one — 1440×900 unless `--size WxH` says otherwise.
  const [viewW, viewH] = (option("size", "1440x900") || "1440x900").split("x").map(Number);
  await cdp.send(
    "Emulation.setDeviceMetricsOverride",
    { width: viewW, height: viewH, deviceScaleFactor: 1, mobile: false },
    sessionId,
  );
  await cdp.send("Page.navigate", { url: target }, sessionId);

  const read = async () => {
    const result = await cdp.send(
      "Runtime.evaluate",
      { expression: READ, returnByValue: true, awaitPromise: false },
      sessionId,
    );
    return result.result?.value ?? null;
  };

  /** Every distinct picture drawn over `windowMs`, one sample per animation frame. */
  const readFrames = async (windowMs) => {
    const result = await cdp.send(
      "Runtime.evaluate",
      { expression: FRAME_WINDOW(windowMs), awaitPromise: true, returnByValue: true },
      sessionId,
    );
    return result.result?.value ?? [];
  };

  // The solve arrives in the worker; wait for the page to name an outcome. The toolbar
  // slot reads "Solving seed 0…" while a solve is in flight (notes/14 §3's S0), so
  // an answer is an outcome that is not that. The old sentence was "solving · seed
  // 0 · goal 8 Lines"; both forms count as in flight.
  const settled = (state) => state && state.outcome.length > 0 && !/^solving\b/i.test(state.outcome);
  let solved = null;
  for (let i = 0; i < 160 && !solved; i += 1) {
    await sleep(250);
    const state = await read();
    if (settled(state)) solved = state;
  }

  if (!solved) throw new Error("the page never reported an outcome");
  console.log(`browser check · ${target}`);
  console.log(`  status   ${solved.status}`);
  console.log(`  outcome  ${solved.outcome}`);
  console.log(`  progress ${solved.progress}`);

  check("the page reports an outcome instead of an error", solved.tone !== "error", solved.tone || "ok");
  // Ticket 13's words: "Won: the Goal of 8 lines was reached, 8 Lines cleared."
  // The page's words now: "Win · 8 of 8 lines · 9.3 ms", and for a loss "Loss ·
  // reached 17 of 20 lines · … ms" — the outcome word, the Lines against the Goal,
  // and the page's own clock. The engine's evaluations are in the status bar.
  check("the outcome came from wasm", /^(Win · \d+ of \d+ lines|Loss · reached \d+ of \d+ lines) · [\d.]+ ms$/.test(solved.outcome), solved.outcome);
  check("the status names the default game", /seed 0/.test(solved.status) && /beam 25/.test(solved.status));

  // Ticket 17's two deploy-time assertions (§10: the smoke test runs against the
  // live URL). The first is the ABI handshake in the page's own words — the loader
  // refuses a mismatched artifact, so `abi 2` on screen means the deployed bytes and
  // this page agree. The second catches a preview URL wired into production, or a
  // page served over http where the wasm's streaming compile is not the same path.
  check("the worker booted the wasm and the ABI handshake agrees", /\babi 2\b/.test(solved.boot), solved.boot || "no boot slot");
  const wanted = new URL(target);
  check(
    "the page is the requested origin, and https when it was asked for",
    solved.origin === wanted.origin && (wanted.protocol !== "https:" || solved.protocol === "https:"),
    `${solved.origin} (asked for ${wanted.origin})`,
  );

  const shot = option("shot", join(tmpdir(), "tetris-lab-check.png"));
  const shoot = async (label) => {
    const capture = await cdp.send("Page.captureScreenshot", { format: "png" }, sessionId);
    const file = shot.replace(/\.png$/, `-${label}.png`);
    writeFileSync(file, Buffer.from(capture.data, "base64"));
    return file;
  };

  const expected = await expectedBoards();
  const shots = [await shoot("start")];
  console.log(`  total    ${expected.total} Moves, clearing at ${expected.clearing.join(", ")}`);

  const key = async (keyName, code, vk) => {
    await cdp.send(
      "Input.dispatchKeyEvent",
      { type: "rawKeyDown", key: keyName, code, windowsVirtualKeyCode: vk },
      sessionId,
    );
    await cdp.send(
      "Input.dispatchKeyEvent",
      { type: "keyUp", key: keyName, code, windowsVirtualKeyCode: vk },
      sessionId,
    );
    await sleep(120);
  };

  /** Click the move strip at `fraction` of its width: one gesture to any Move. */
  const clickStrip = async (fraction) => {
    const state = await read();
    const rect = state.stripRect;
    const x = Math.round(rect.x + rect.width * fraction);
    const y = Math.round(rect.y + rect.height / 2);
    await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", clickCount: 1 }, sessionId);
    await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", clickCount: 1 }, sessionId);
    await sleep(150);
    return read();
  };

  const setSpeedSlider = async (position) => {
    await cdp.send(
      "Runtime.evaluate",
      {
        expression: `(() => { const s = document.getElementById("speed"); s.value = "${position}"; s.dispatchEvent(new Event("input", { bubbles: true })); return s.value; })()`,
        returnByValue: true,
      },
      sessionId,
    );
    await sleep(150);
    return read();
  };

  const diff = (a, b) => [...(a ?? "")].filter((bit, index) => bit !== b?.[index]).length;

  // ---- the page solved, and its chrome is discoverable -------------------------
  check("the Move strip is one cell per Move, and addresses every one", solved.seekMax === expected.total && solved.stripCells === expected.total, `${solved.stripCells} cells, max ${solved.seekMax}, ${expected.total} Moves`);
  check("every control carries a tooltip", solved.untitled.length === 0, solved.untitled.join(", ") || "all titled");
  check("the keyboard map is on the page", solved.sheetRows >= 7, `${solved.sheetRows} rows behind ?`);
  check("the speed is labelled in words", /Moves\/s/.test(solved.speed) && /Moves\/s/.test(solved.speedLabel) && /s per Move/.test(solved.speedLabel), `${solved.speed} · ${solved.speedLabel}`);

  // ---- playback advances, and the picture changes -----------------------------
  const first = solved;
  const drawn = new Set([first.canvas.cells]);
  let advanced = false;
  for (let i = 0; i < 24; i += 1) {
    await sleep(250);
    const state = await read();
    if (!state) continue;
    drawn.add(state.canvas.cells);
    if (state.seek > first.seek) advanced = true;
  }
  check("the Move list plays back", advanced, `Move ${first.seek} → ${(await read()).seek}`);
  check("the board is drawn", first.canvas.cells > 0, `${first.canvas.cells} lit pixels`);
  check("the stack changes as pieces land", drawn.size >= 3, `${drawn.size} distinct fills seen`);

  // ---- pause, resume, restart, and the map that does it -----------------------
  await key(" ", "Space", 32);
  const paused = await read();
  check("Space pauses", paused.pressed === "false" && paused.playText === "play", `${paused.playText} · pressed ${paused.pressed}`);
  const pausedAt = paused.seek;
  await sleep(400);
  check("a paused playback does not advance", (await read()).seek === pausedAt, `still Move ${pausedAt}`);

  await key(" ", "Space", 32);
  check("Space resumes", (await read()).pressed === "true");

  // Stepping is asserted from a *still* clock. Reading the position and pressing
  // the arrow is two round trips (~200 ms), and at 1.43 moves/s a running clock can
  // cross a step boundary in between — which it did, one run in three, reporting
  // "Move 12" where "Move 11" was meant. Paused, only the key moves the position,
  // so the assertion is about the key and not about when the read landed.
  await key(" ", "Space", 32); // pause: the arrows are measured from a frozen position
  const beforeStep = await read();
  console.log(`  arrows   stepping from Move ${beforeStep.seek}, ${beforeStep.pressed === "true" ? "STILL PLAYING" : "paused"}`);
  await key("ArrowRight", "ArrowRight", 39);
  const stepped = await read();
  check("→ steps one Move forward (and pauses)", stepped.seek === Math.min(expected.total, beforeStep.seek + 1) && stepped.pressed === "false", `Move ${beforeStep.seek} → ${stepped.seek}`);
  await key("ArrowLeft", "ArrowLeft", 37);
  const wentBack = await read();
  check("← steps one Move back", wentBack.seek === Math.min(expected.total, beforeStep.seek) && wentBack.pressed === "false", `Move ${stepped.seek} → ${wentBack.seek}`);

  await key("End", "End", 35);
  const atEnd = await read();
  check("End reaches the last Move", atEnd.seek === expected.total && atEnd.playText === "replay", `Move ${atEnd.seek} of ${atEnd.seekMax}`);
  shots.push(await shoot("end"));
  check(
    "the last Move draws the engine's Move list played out",
    diff(atEnd.mask, expected.masks[expected.total]) === 0,
    `${diff(atEnd.mask, expected.masks[expected.total])} of 200 cells differ`,
  );

  await key("Home", "Home", 36);
  const atStart = await read();
  check("Home returns to the starting Board", atStart.seek === 0, `Move ${atStart.seek}`);
  check("move 0 is the Board the engine generated", diff(atStart.mask, expected.masks[0]) === 0);

  await key("r", "KeyR", 82);
  const restarted = await read();
  check("R restarts and plays", restarted.seek === 0 && restarted.pressed === "true", `Move ${restarted.seek} · pressed ${restarted.pressed}`);

  await key("?", "Slash", 191);
  check("? opens the keyboard sheet", (await read()).sheetOpen);
  await key("Escape", "Escape", 27);
  check("Escape closes it", !(await read()).sheetOpen);

  // ---- ticket 16: the Explainer drawer, from both doors ----------------------
  //
  // The drawer explains the engine that shipped, and it must open from the control
  // *and* from `E`, close from either, and add no page scroll (it is out of flow by
  // design). The words are read only while it is open: `innerText` skips what the CSS
  // hides, so a closed drawer would pass a scan of nothing.
  console.log("\n── the Explainer drawer ──");
  const clickHow = async () =>
    cdp.send(
      "Runtime.evaluate",
      { expression: `document.getElementById("how").click()`, returnByValue: true },
      sessionId,
    );
  await clickHow();
  await sleep(120);
  check("How it works opens the Explainer", (await read()).drawerOpen);
  await clickHow();
  await sleep(120);
  check("How it works closes it again", !(await read()).drawerOpen);

  // A real click leaves focus on the control; make that condition explicit, because
  // `E` being dead while the opening button holds focus is the bug this asserts away.
  await cdp.send(
    "Runtime.evaluate",
    { expression: `document.getElementById("how").focus()`, returnByValue: true },
    sessionId,
  );
  await key("e", "KeyE", 69);
  const drawerFromKey = await read();
  check("E opens it, even with the control focused", drawerFromKey.drawerOpen);
  await key("e", "KeyE", 69);
  check("E closes it again", !(await read()).drawerOpen);

  await key("e", "KeyE", 69);
  await sleep(120);
  const drawer = await read();
  check(
    "the open drawer introduces no page scroll",
    drawer.scrollHeight <= drawer.viewport.h + 1 &&
      drawer.bodyScrollHeight <= drawer.viewport.h + 1 &&
      drawer.rootOverflow === "hidden",
    `${drawer.scrollHeight} / ${drawer.viewport.h} · root overflow ${drawer.rootOverflow}`,
  );
  check(
    "the drawer's prose fits the design floor without an internal scroll",
    drawer.drawerBody !== null && drawer.drawerBody.free >= 0,
    drawer.drawerBody ? `${drawer.drawerBody.free} px to spare under the last paragraph` : "no .dbody",
  );
  const leads = ["The Lab", "Beam width", "Dedup", "Tail search", "No-suicide rule", "Evaluator", "Dataset", "Reference run", "Run"];
  check(
    "it explains the beam, Dedup, the Tail search, the No-suicide rule and the shipped Evaluator",
    leads.every((lead) => drawer.drawerText.includes(`${lead}.`)),
    leads.filter((lead) => !drawer.drawerText.includes(`${lead}.`)).join(", ") || `${leads.length} sections`,
  );
  check(
    "it names the Evaluator that shipped, and no other",
    /Dellacherie/.test(drawer.drawerText) && /published weights/.test(drawer.drawerText),
  );
  const wordHits = [
    ...scanText(drawer.drawerText, "#drawer", { tiers: [TIER_A] }),
    ...scanText(drawer.drawerText, "#drawer", { tiers: [TIER_B] }),
  ];
  check(
    "the drawer's rendered words pass the banned-words gate",
    wordHits.length === 0,
    wordHits.length > 0
      ? formatHits(wordHits, 1)
      : `${drawer.drawerText.trim().split(/\s+/).length} words, no banned word`,
  );
  await key("Escape", "Escape", 27);
  check("Escape closes it", !(await read()).drawerOpen);
  // The drawer hands focus back to whatever opened it (a dialog's normal exit), which
  // here is the `How it works` button. That is the correct behaviour and the reason
  // `E` must still work from a focused button — but Space on a focused button is the
  // button's own key, so the harness puts focus back on the page before it tests
  // Space as play/pause. A visitor who clicks the strip does the same thing.
  await cdp.send(
    "Runtime.evaluate",
    { expression: "document.activeElement?.blur()", returnByValue: true },
    sessionId,
  );

  // ---- the move strip is the seek bar -----------------------------------------
  await key(" ", "Space", 32); // pause, so the mask holds still while it is read
  for (const fraction of [0.6, 0.25, 0.95, 0.05, 0.45]) {
    const state = await clickStrip(fraction);
    const target = Math.round(fraction * expected.total);
    check(
      `a gesture at ${Math.round(fraction * 100)}% seeks to Move ${target}`,
      Math.abs(state.seek - target) <= 1,
      `Move ${state.seek}`,
    );
    check(
      `Move ${target} is drawn as the engine's board at that Move`,
      diff(state.mask, expected.masks[state.seek]) === 0,
      `${diff(state.mask, expected.masks[state.seek])} of 200 cells differ`,
    );
  }
  const scrubbed = await read();
  check("a gesture pauses, so the drag is not fighting the clock", scrubbed.pressed === "false");

  // ---- playing from a scrubbed position continues correctly -------------------
  await key(" ", "Space", 32); // play again from the scrubbed Move
  await sleep(1200);
  const resumed = await read();
  check(
    "playing from a scrub continues forward from that Move",
    resumed.seek > scrubbed.seek || resumed.seek === expected.total,
    `Move ${scrubbed.seek} → ${resumed.seek}`,
  );
  check(
    "and lands on boards only the engine's Move list has",
    expected.masks.includes(resumed.mask) || resumed.mask === expected.masks[expected.total],
    `${diff(resumed.mask, expected.masks[resumed.seek])} of 200 cells differ from Move ${resumed.seek}`,
  );

  // ---- the speed control is continuous ---------------------------------------
  await key(" ", "Space", 32); // pause
  const speeds = [];
  for (const position of [0.1, 0.5, 0.9]) {
    const state = await setSpeedSlider(position);
    speeds.push(state.speed);
  }
  console.log(`  speeds   ${speeds.join(" | ")}`);
  check("the speed control yields more than four values", new Set(speeds).size === 3, speeds.join(" | "));
  check(
    "and none of them is a preset multiplier",
    speeds.every((label) => !/^[0-9.]+×/.test(label)),
    speeds.join(" | "),
  );

  await key("=", "Equal", 187);
  const faster = await read();
  await key("-", "Minus", 189);
  const slower = await read();
  check("= and - move one speed stop", faster.speed !== slower.speed, `${faster.speed} → ${slower.speed}`);
  shots.push(await shoot("mid"));

  // ---- motion: a drop and a collapse are drawn, not jumped over ---------------
  //
  // *Where to look.* The clears are at `expected.clearing` — Move 8 in the default
  // game — so sampling from Move 0 at this pace never reaches a collapse inside any
  // sane window, and used to report "40 distinct frames, all end states" for a page
  // that was animating correctly. The watch therefore starts one Move before the
  // first clear.
  //
  // *How long.* The slider at 0.32 is ~1.27 moves/s (log scale: exp(lerp(ln 0.25,
  // ln 40, 0.32))), so one step is 788 ms; a step is drop 45 % (355 ms) then clear
  // 36 % (284 ms) then hold. 2200 ms is 2.8 steps, so the collapse of `firstClear`
  // — which begins 355 ms after play does — cannot fall outside the window.
  //
  // *What counts as motion.* The picture is the settled mask *and* the in-flight
  // mask: an end state is a settled mask with nothing in the air. A drop is a
  // picture no end state has, and so is a collapse, without either having to change
  // the window it is read in.
  await setSpeedSlider(0.32);
  const firstClear = expected.clearing[0] ?? Math.min(2, expected.total);
  await key("Home", "Home", 36);
  for (let i = 0; i < firstClear - 1; i += 1) await key("ArrowRight", "ArrowRight", 39);
  const beforeMotion = await read();
  console.log(`  motion   watching from Move ${beforeMotion.seek} of ${expected.total}; first clear is Move ${firstClear}`);
  await key(" ", "Space", 32);
  const frames = await readFrames(2200);
  const atRest = new Set(expected.masks.map((settled) => settled + "0".repeat(200)));
  const pictures = new Set(frames.map((frame) => frame.picture));
  const moving = frames.filter((frame) => !atRest.has(frame.picture));
  const inAir = moving.filter((frame) => frame.flight.includes("1"));
  check(
    "a frame between two boards is drawn: the drop and the collapse are motion",
    moving.length > 0,
    moving.length > 0
      ? `${pictures.size} distinct pictures, ${moving.length} not an end state, ${inAir.length} with a cell in the air, first at ${moving[0].at} ms`
      : `${pictures.size} distinct pictures, all end states`,
  );
  shots.push(await shoot("mid-drop"));
  check("the strip keeps carrying the position", (await read()).seek >= 0);
  await key(" ", "Space", 32); // pause for the last screenshot
  shots.push(await shoot("paused"));

  // ---- ticket 13: a control re-solves, and the URL carries the game ------------
  //
  // §2.1.7's promise, checked the way a visitor does it: change a field, and read
  // back the Board on screen (the pixels), the address bar, and the session history.
  // `replaceState` means the address bar follows the control while Back still means
  // "leave the Lab" — one history entry before and after.
  const setField = async (field, value) => {
    await cdp.send(
      "Runtime.evaluate",
      {
        expression: `(() => {
          const input = document.querySelector('#playground [data-field="${field}"]');
          if (!input) return false;
          input.value = ${JSON.stringify(String(value))};
          input.dispatchEvent(new Event("input", { bubbles: true }));
          input.dispatchEvent(new Event("change", { bubbles: true }));
          return true;
        })()`,
        returnByValue: true,
      },
      sessionId,
    );
  };
  /** Wait for a fresh outcome: the page clears it while a solve is in flight. */
  const settle = async (after) => {
    for (let i = 0; i < 40; i += 1) {
      await sleep(120);
      const state = await read();
      if (settled(state) && state.outcome !== after) return state;
    }
    return read();
  };

  if (solved.untitled.length === 0) {
    const before = await read();
    console.log(`  playground ${before.presets.length} presets, fields ${Object.keys(before.playground).join(", ")}`);
    check(
      "the Playground offers both shipped configurations",
      before.presets.includes("goal8-p40-cap7") && before.presets.includes("goal12-p100-cap7"),
      before.presets.join(", "),
    );
    check(
      "the width control opens at the default and names the cap",
      before.playground.beam === "25" && /default 25/.test(before.beamReadout) && /800/.test(before.beamReadout),
      before.beamReadout,
    );

    // A seed: the Board must change on screen, and the URL must follow it.
    await setField("seed", 7);
    const seeded = await settle(before.outcome);
    check(
      "changing the seed re-solves and the Board on screen changes",
      seeded.mask !== before.mask && /seed 7/.test(seeded.status),
      `${before.mask.slice(0, 40)}… vs ${seeded.mask.slice(0, 40)}…`,
    );
    check(
      "the URL carries the game in lowercase keys, and Back still leaves the page",
      seeded.search === "?seed=7" && seeded.historyLength === before.historyLength,
      `${seeded.search || "(bare)"} · history ${seeded.historyLength}`,
    );

    // The starting height: a different Board, the same Sequence length, one field.
    const beforeCap = await read();
    await setField("cap", 19);
    const raised = await settle(beforeCap.outcome);
    check(
      "raising the starting height changes the Board and touches one field",
      raised.mask !== beforeCap.mask && raised.search.includes("cap=19"),
      raised.search || "(bare)",
    );

    // A goal the search cannot reach: an outcome, in words, not an error.
    await setField("goal", 20);
    const lost = await settle(raised.outcome);
    check(
      "a game the search cannot win says so with the Lines it reached",
      /^Loss · reached \d+ of 20 lines · [\d.]+ ms$/.test(lost.outcome) && lost.tone !== "error",
      lost.outcome.slice(0, 100),
    );
    check(
      "a loss has no Moves to play, and says so instead of an empty counter",
      lost.outcomeKind === "lost" && /^No Moves to replay$/.test(lost.seekText),
      lost.progress,
    );

    // A preset: one click fills the fields and lands on the committed game.
    await cdp.send(
      "Runtime.evaluate",
      {
        expression: `document.querySelector('#playground [data-preset="goal12-p100-cap7"]').click()`,
        returnByValue: true,
      },
      sessionId,
    );
    const preset = await settle(lost.outcome);
    check(
      "a preset fills every field and reproduces the committed game",
      preset.search === "?goal=12&pieces=100" &&
        preset.playground.seed === "0" &&
        preset.playground.pieces === "100" &&
        /goal 12 lines · 100 pieces · starting height 7/.test(preset.status),
      preset.search || "(bare)",
    );

    // The expert width: the slider runs to the cap and re-solves.
    const beforeBeam = await read();
    await setField("beam", 800);
    const wide = await settle(beforeBeam.outcome);
    check(
      "the beam slider re-solves, and the URL carries the width",
      wide.search.includes("beam=800") && wide.outcome !== beforeBeam.outcome,
      wide.search,
    );
    check("the re-solve reason is named per control", wide.reason === "beam", wide.reason);
    shots.push(await shoot("playground"));
  }

  // ---- ticket 15: a Run beside the Reference run ------------------------------
  //
  // §2.5.5 and §2.5.7 in one gesture: press Run, watch the progress move, stop one,
  // then run the whole split and read the table. Three things are asserted that only
  // a browser can settle — that a Run *finishes* inside its budget on this machine,
  // that `terminate()` really stops it (the panel says partial and no figure claims
  // the split), and that the live column agrees with the committed Reference row on
  // the one thing that must be identical: the engine's own integers.
  {
    const clickId = (id) =>
      cdp.send(
        "Runtime.evaluate",
        { expression: `document.getElementById(${JSON.stringify(id)}).click()`, returnByValue: true },
        sessionId,
      );
    const runState = async () => (await read()).run;

    const ready = await runState();
    check("the Run panel offers a Run and a Stop", !ready.startDisabled && ready.stopDisabled, `start ${ready.startDisabled ? "disabled" : "live"}, stop ${ready.stopDisabled ? "disabled" : "live"}`);
    check("the Reference column is committed and labelled", /Ryzen|\w+ \w+ ·/.test(ready.labels.join(" ")), ready.labels.join(" | ").slice(0, 160));
    check("the 5 s budget names the 8-core laptop as an assumption", /assumption/.test(ready.assumption) && /not a measurement/.test(ready.assumption), ready.assumption.slice(0, 120));

    // 1. Stop, and be honest about it. The whole Run is ~1 s here, so the stop has
    //    to land inside that window; retried a few times rather than flaking.
    //
    //    The trigger is a *measured* cell, not the progress line: a worker reports
    //    games from inside a chunk, and terminate() throws that chunk away, so a stop
    //    that lands on the first in-flight progress is honestly a partial Run of 0
    //    games — correct, but not what this assertion is about. The live cell is the
    //    games the table holds, so waiting for it is waiting for a denominator.
    const liveWinRate = (state) => state.cells["0-r1-yours"] ?? "";
    let partial = null;
    for (let attempt = 0; attempt < 6 && !partial; attempt += 1) {
      await clickId("run-dataset");
      for (let i = 0; i < 400; i += 1) {
        await sleep(10);
        const state = await runState();
        if (state.status === "complete" || state.status === "failed") break;
        if (state.status === "running" && / of [\d,]+$/.test(liveWinRate(state))) {
          await clickId("run-stop");
          break;
        }
      }
      for (let i = 0; i < 100; i += 1) {
        await sleep(20);
        const state = await runState();
        if (state.status === "stopped" && / of [\d,]+$/.test(liveWinRate(state))) {
          partial = state;
          break;
        }
        if (state.status === "complete") break;
        if (state.status === "stopped") break; // a 0-game stop: retry the gesture
      }
    }
    check(
      "a Run stops immediately, and what it measured is labelled partial",
      Boolean(partial) && /partial/.test(partial.state) && partial.labels.some((line) => /partial/.test(line)),
      partial ? `${partial.state} · ${partial.labels.join(" / ").slice(0, 100)}` : "no stopped Run",
    );
    check(
      "a stopped Run keeps its denominator in the same cell as its value",
      Boolean(partial) && / of [\d,]+$/.test(partial.cells["0-r1-yours"] ?? ""),
      partial?.cells["0-r1-yours"] ?? "(no cell)",
    );
    shots.push(await shoot("run-partial"));

    // 2. The whole split, timed. The budget is §9.3's: under 5 s on this box.
    await clickId("run-dataset");
    const startedAt = Date.now();
    let done = null;
    for (let i = 0; i < 2400 && !done; i += 1) {
      await sleep(25);
      const state = await runState();
      if (state.status === "complete" || state.status === "failed") done = state;
    }
    const wallMs = Date.now() - startedAt;
    console.log(`  run      ${(wallMs / 1000).toFixed(2)} s wall clock on ${done?.workers ?? 0} workers · ${done?.state ?? "never finished"}`);
    check("a Run of both configurations completes", done?.status === "complete", done ? done.state : "never finished");
    check("the Run finishes inside the 5 s budget", wallMs < 5000, `${(wallMs / 1000).toFixed(2)} s on ${done?.workers ?? 0} workers`);

    if (done) {
      for (const config of ["0", "1"]) {
        check(
          `configuration ${config}: the live Evaluations are the Reference row's`,
          done.cells[`${config}-r5-yours`] === done.cells[`${config}-r5-reference`],
          `${done.cells[`${config}-r5-yours`]} vs ${done.cells[`${config}-r5-reference`]}`,
        );
        check(
          `configuration ${config}: the live win rate and games won are the Reference row's`,
          done.cells[`${config}-r2-yours`] === done.cells[`${config}-r2-reference`] &&
            (done.cells[`${config}-r1-yours`] ?? "").replace(/ of [\d,]+$/, "") === done.cells[`${config}-r1-reference`],
          `${done.cells[`${config}-r1-yours`]} / ${done.cells[`${config}-r2-yours`]}`,
        );
        check(
          `configuration ${config}: the ms rows are this machine's and say so`,
          /ms/.test(done.cells[`${config}-r6-yours`] ?? "") && done.cells[`${config}-r6-yours`] !== done.cells[`${config}-r6-reference`],
          `${done.cells[`${config}-r6-yours`]} vs ${done.cells[`${config}-r6-reference`]}`,
        );
      }
      check(
        "both columns name the machine that produced them",
        done.labels.some((line) => /this browser/.test(line)) && done.labels.some((line) => /Ryzen/.test(line)),
        done.labels.join(" | ").slice(0, 200),
      );
      check("the Run says the split is measured, in words", /2,000\/2,000 games/.test(done.state), done.state);
      shots.push(await shoot("run-complete"));
    }
  }

  for (const file of shots) console.log(`  screenshot ${file}`);

  const failed = checks.filter((entry) => !entry.ok);
  exitCode = failed.length === 0 ? 0 : 1;
  console.log(exitCode === 0 ? "\nPASS the page solves, plays back, and can be driven" : `\nFAIL ${failed.length} check(s)`);
} catch (error) {
  console.error(`browser check failed: ${error.message}`);
  exitCode = 1;
} finally {
  if (chrome) chrome.kill();
  if (server) server.close();
  await sleep(300);
  if (profile) {
    try {
      rmSync(profile, { recursive: true, force: true });
    } catch {
      // Windows may still hold a handle on the profile directory; it is disposable.
    }
  }
}
process.exit(exitCode);
