#!/usr/bin/env node
// Headless acceptance for the page: open it in a real browser, watch a real wasm
// instance solve a real game, and check the board actually plays back.
//
//   node scripts/browser-check.mjs                 # serves dist/ itself
//   node scripts/browser-check.mjs --url https://… # against a live URL
//   node scripts/browser-check.mjs --shot /tmp/tetris.png
//
// No dependencies: a static file server, then the Chrome DevTools Protocol over
// Node's built-in WebSocket (the shape `host-probe/browser-check.mjs` established).
// It asserts what a test can honestly assert about a canvas — that the page solved,
// that playback advances, that the drawn stack changes as pieces land and rows
// collapse — and writes a screenshot for the judgment a test cannot make.
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
 * What the page should end up showing, computed here from the artifact: generate
 * the same default game, solve it, and play the Move list out on the field module.
 * The page never sees this; it is the yardstick the drawn pixels are held against.
 */
async function expectedFinalMask() {
  const { pathToFileURL } = await import("node:url");
  const load = (path) => import(pathToFileURL(path).href);
  const { loadEngineFromBytes } = await load(resolve(WEB, "src", "core", "loader.ts"));
  const { fieldFrom, place, restShift, clearFullRows } = await load(resolve(WEB, "src", "ui", "field.ts"));
  const { rotationOf } = await load(resolve(WEB, "src", "ui", "pieces.ts"));

  const bytes = new Uint8Array(readFileSync(ARTIFACT));
  const engine = await loadEngineFromBytes(bytes);
  engine.genGame(0, 40, 7);
  const field = fieldFrom(engine.board());
  const outcome = engine.solve(8, 25);
  const moves = engine.moves(outcome.moveCount);
  for (const word of moves) {
    const shape = rotationOf((word >>> 16) & 0xff, (word >>> 8) & 0xff);
    const shift = restShift(field, shape, word & 0xff);
    place(field, shape, word & 0xff, shift);
    clearFullRows(field);
  }
  let mask = "";
  for (let row = 0; row < 20; row += 1) {
    for (let col = 0; col < 10; col += 1) {
      mask += field[col] & (1 << (19 - row)) ? "1" : "0";
    }
  }
  return { mask, outcome };
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
  return {
    status: text("status"),
    outcome: text("outcome"),
    progress: text("progress"),
    tone: document.getElementById("status")?.dataset.tone ?? "",
    mask,
    canvas: { width: canvas?.width ?? 0, height: canvas?.height ?? 0, ...drawn },
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

  // Playback: the progress line must advance, and the drawn stack must change as
  // pieces land and rows collapse. A sample is taken every 300 ms, and the middle
  // one is written out so a human can see a drop in flight.
  const first = solved;
  const drawn = new Set([first.canvas.cells]);
  let advanced = false;
  let middle = { progress: "", canvas: first.canvas };
  const shots = [await shoot("start")];
  for (let i = 0; i < 24; i += 1) {
    await sleep(300);
    const state = await read();
    if (!state) continue;
    drawn.add(state.canvas.cells);
    if (state.progress !== first.progress && state.progress.length > 0) advanced = true;
    if (i === 8) {
      middle = state;
      shots.push(await shoot("mid"));
    }
  }
  shots.push(await shoot("end"));
  console.log(`  mid-point ${middle.progress}`);

  // Playback holds the finished board for a beat before it starts over, so the
  // settled mask can be read back and compared with the engine's own Move list.
  let finalMask = null;
  for (let i = 0; i < 40 && finalMask === null; i += 1) {
    const state = await read();
    const played = /move (\d+) of (\d+)/.exec(state?.progress ?? "");
    if (played && played[1] === played[2] && state.canvas.cells > 0) finalMask = state.mask;
    else await sleep(200);
  }
  const expected = await expectedFinalMask();
  if (finalMask === null) {
    check("the page draws the settled board", false, "playback never reported its last move");
  } else {
    const wrong = [...expected.mask].filter((bit, i) => bit !== finalMask[i]).length;
    check(
      "the settled board on screen is the engine's Move list played out",
      wrong === 0,
      wrong === 0 ? "200 cells agree" : `${wrong} of 200 cells differ`,
    );
  }

  check("the Move list plays back", advanced, first.progress);
  check("playback leaves the starting position", middle.canvas.cells !== first.canvas.cells, middle.progress);
  check("the board is drawn", first.canvas.cells > 0, `${first.canvas.cells} lit pixels`);
  check(
    "the stack changes as pieces land",
    drawn.size >= 3,
    `${drawn.size} distinct fills seen`,
  );

  for (const file of shots) console.log(`  screenshot ${file}`);

  const failed = checks.filter((entry) => !entry.ok);
  exitCode = failed.length === 0 ? 0 : 1;
  console.log(exitCode === 0 ? "\nPASS the page solves and plays back" : `\nFAIL ${failed.length} check(s)`);
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
