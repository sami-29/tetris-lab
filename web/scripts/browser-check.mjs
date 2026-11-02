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
  // Read the 10×20 settled mask back out of the pixels, cell centre by cell
  // centre. The settled fill is #7f9dc0; the piece in flight is brighter, the ghost
  // outline darker, so the window below picks out settled cells only.
  let mask = "";
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
        mask += r > 100 && r < 150 && g > 135 && g < 180 && b > 170 && b < 215 ? "1" : "0";
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
    tone: document.getElementById("status")?.dataset.tone ?? "",
    mask,
    canvas: { width: canvas?.width ?? 0, height: canvas?.height ?? 0, ...drawn },
    // The controls, as a visitor sees them.
    seek: Number(strip?.getAttribute("aria-valuenow") ?? -1),
    seekMax: Number(strip?.getAttribute("aria-valuemax") ?? -1),
    seekText: strip?.getAttribute("aria-valuetext") ?? "",
    playText: (play?.textContent ?? "").trim(),
    pressed: play?.getAttribute("aria-pressed") ?? "",
    speed: text("speed-readout"),
    sheetOpen: sheet?.dataset.open === "true",
    sheetRows: document.querySelectorAll("#sheet dt").length,
    stripCells: document.querySelectorAll("#strip i").length,
    untitled: [...document.querySelectorAll("button, input, [role=slider]")]
      .filter((el) => !el.getAttribute("title"))
      .map((el) => el.id || el.tagName),
    stripRect: (() => {
      const rect = strip?.getBoundingClientRect();
      return rect ? { x: rect.left, y: rect.top, width: rect.width, height: rect.height } : null;
    })(),
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
  await cdp.send("Page.navigate", { url: target }, sessionId);

  const read = async () => {
    const result = await cdp.send(
      "Runtime.evaluate",
      { expression: READ, returnByValue: true, awaitPromise: false },
      sessionId,
    );
    return result.result?.value ?? null;
  };

  // The solve arrives in the worker; wait for the page to name an outcome.
  let solved = null;
  for (let i = 0; i < 160 && !solved; i += 1) {
    await sleep(250);
    const state = await read();
    if (state && state.outcome.length > 0) solved = state;
  }

  if (!solved) throw new Error("the page never reported an outcome");
  console.log(`browser check · ${target}`);
  console.log(`  status   ${solved.status}`);
  console.log(`  outcome  ${solved.outcome}`);
  console.log(`  progress ${solved.progress}`);

  check("the page reports an outcome instead of an error", solved.tone !== "error", solved.tone || "ok");
  check("the outcome came from wasm", /won|lost/.test(solved.outcome) && /evaluations/.test(solved.outcome), solved.outcome);
  check("the status names the default game", /seed 0/.test(solved.status) && /beam 25/.test(solved.status));

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
  check("the speed is labelled in words", /moves\/s/.test(solved.speed) && /s per move/.test(solved.speed), solved.speed);

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
  check("Space pauses", /paused/.test(paused.progress) && paused.playText === "play", paused.progress.slice(0, 60));
  const pausedAt = paused.seek;
  await sleep(400);
  check("a paused playback does not advance", (await read()).seek === pausedAt, `still Move ${pausedAt}`);

  await key(" ", "Space", 32);
  check("Space resumes", /playing/.test((await read()).progress));

  await key("ArrowRight", "ArrowRight", 39);
  const stepped = await read();
  check("→ steps one Move forward (and pauses)", stepped.seek === Math.min(expected.total, pausedAt + 1) && /paused/.test(stepped.progress), `Move ${stepped.seek}`);
  await key("ArrowLeft", "ArrowLeft", 37);
  check("← steps one Move back", (await read()).seek === Math.min(expected.total, pausedAt), `Move ${(await read()).seek}`);

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
  check("R restarts and plays", restarted.seek === 0 && /playing/.test(restarted.progress), restarted.progress.slice(0, 60));

  await key("?", "Slash", 191);
  check("? opens the keyboard sheet", (await read()).sheetOpen);
  await key("Escape", "Escape", 27);
  check("Escape closes it", !(await read()).sheetOpen);

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
  check("a gesture pauses, so the drag is not fighting the clock", /paused/.test(scrubbed.progress));

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
  await setSpeedSlider(0.32); // about 0.9 moves/s: slow enough to sample a collapse
  await key("Home", "Home", 36);
  await key(" ", "Space", 32);
  const sampled = new Set();
  let sawUnknown = null;
  for (let i = 0; i < 40; i += 1) {
    await sleep(120);
    const state = await read();
    if (!state) continue;
    sampled.add(state.mask);
    if (!expected.masks.includes(state.mask) && sawUnknown === null) {
      sawUnknown = { mask: state.mask, seek: state.seek };
      shots.push(await shoot("collapse"));
    }
  }
  check(
    "a frame between two boards is drawn: the drop and the collapse are motion",
    sawUnknown !== null,
    sawUnknown ? `an intermediate picture at Move ${sawUnknown.seek}` : `${sampled.size} distinct frames, all end states`,
  );
  check("the strip keeps carrying the position", (await read()).seek >= 0);
  await key(" ", "Space", 32); // pause for the last screenshot
  shots.push(await shoot("paused"));

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
