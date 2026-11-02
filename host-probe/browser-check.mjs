#!/usr/bin/env node
// Browser half of the ticket 09 proof: loads the probe page in headless Chrome
// and reads what the module worker reported (MIME, cache, abi_version, compute).
//
//   node browser-check.mjs http://127.0.0.1:8791
//   CHROME_PATH=/path/to/chrome node browser-check.mjs https://lab.example.com
//
// No dependencies: Chrome DevTools Protocol over Node's built-in WebSocket.
// verify.sh proves what the server sends; this proves a worker executes it.

import { spawn } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const target = process.argv[2];
if (!target) {
  console.error("usage: node browser-check.mjs <url>");
  process.exit(2);
}

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

const chromePath = CHROME_CANDIDATES.find((p) => existsSync(p));
if (!chromePath) {
  console.error("no Chrome/Edge found; set CHROME_PATH");
  process.exit(2);
}

const port = 9333 + Math.floor(Math.random() * 200);
const profile = mkdtempSync(join(tmpdir(), "probe-chrome-"));

const chrome = spawn(
  chromePath,
  [
    "--headless=new",
    "--disable-gpu",
    "--no-first-run",
    "--no-default-browser-check",
    `--remote-debugging-port=${port}`,
    `--user-data-dir=${profile}`,
    "about:blank",
  ],
  { stdio: "ignore" },
);

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function debuggerUrl() {
  for (let i = 0; i < 100; i++) {
    try {
      const res = await fetch(`http://127.0.0.1:${port}/json/version`);
      if (res.ok) return (await res.json()).webSocketDebuggerUrl;
    } catch {
      // not listening yet
    }
    await sleep(100);
  }
  throw new Error("Chrome devtools endpoint never came up");
}

function connect(url) {
  const ws = new WebSocket(url);
  const pending = new Map();
  let nextId = 0;
  ws.addEventListener("message", (ev) => {
    let msg;
    try {
      msg = JSON.parse(ev.data);
    } catch {
      return; // not a CDP frame; nothing to resolve
    }
    const entry = pending.get(msg.id);
    if (!entry) return;
    pending.delete(msg.id);
    if (msg.error) entry.reject(new Error(`${msg.error.message} (${msg.error.code})`));
    else entry.resolve(msg.result);
  });
  const ready = new Promise((resolve, reject) => {
    ws.addEventListener("open", resolve, { once: true });
    ws.addEventListener("error", () => reject(new Error("websocket failed")), { once: true });
  });
  const send = (method, params = {}, sessionId) =>
    new Promise((resolve, reject) => {
      const id = ++nextId;
      pending.set(id, { resolve, reject });
      ws.send(JSON.stringify(sessionId ? { id, method, params, sessionId } : { id, method, params }));
    });
  return { ws, ready, send };
}

const READ = `(() => {
  const cell = (id) => document.getElementById(id)?.textContent ?? "";
  return {
    verdict: cell("verdict"),
    pass: document.getElementById("verdict")?.classList.contains("pass") ?? false,
    rows: {
      status: cell("f-status"),
      contentType: cell("f-ctype"),
      cacheControl: cell("f-cache"),
      contentEncoding: cell("f-encoding"),
      bytes: cell("f-bytes"),
      abiVersion: cell("f-abi"),
      add: cell("f-sum"),
      probeBits: cell("f-bits"),
      timing: cell("f-timing"),
      crossOriginIsolated: cell("f-coi"),
    },
  };
})()`;

let exitCode = 1;
try {
  const cdp = connect(await debuggerUrl());
  await cdp.ready;
  const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
  await cdp.send("Page.enable", {}, sessionId);
  await cdp.send("Page.navigate", { url: target }, sessionId);

  let state = null;
  for (let i = 0; i < 120; i++) {
    await sleep(250);
    const res = await cdp.send(
      "Runtime.evaluate",
      { expression: READ, returnByValue: true, awaitPromise: false },
      sessionId,
    );
    state = res.result?.value;
    if (!state) continue;
    // The page reports "waiting on the worker…" until the worker answers.
    if (!state.verdict.startsWith("waiting")) break;
  }

  if (!state) throw new Error("page never produced a verdict element");
  const rows = state.rows;
  console.log(`browser probe · ${target}`);
  for (const [k, v] of Object.entries(rows)) console.log(`  ${k.padEnd(20)} ${v}`);
  console.log(`  ${"verdict".padEnd(20)} ${state.verdict}`);
  if (state.pass) {
    console.log("\nPASS worker ran the module");
    exitCode = 0;
  } else {
    console.log("\nFAIL the worker did not report a passing page");
  }
} catch (err) {
  console.error("browser check failed:", err.message);
} finally {
  chrome.kill();
  await sleep(300);
  try {
    rmSync(profile, { recursive: true, force: true });
  } catch {
    // Windows may still hold a handle; the temp dir is disposable.
  }
}
process.exit(exitCode);
