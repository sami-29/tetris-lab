#!/usr/bin/env node
// Look at the page: one screenshot, optionally one measurement, from a URL.
//
//   node scripts/shot.mjs --url https://lab.samibentaleb.com --viewport 1440x900 \
//     --out shot.png --eval "document.getElementById('strip').children.length"
//
// `browser-check.mjs` is the acceptance pass: it drives the whole page, asserts the
// numbers and takes a few minutes. This is the other thing: what a reviewer needs
// before judging a control's height or a caption's wrap, in about eight seconds, with
// the same Chrome and the same DevTools protocol.
//
// `--eval` runs in the page after load and prints its value as JSON, so one call can
// carry both the picture and the measurement that explains it. The expression is a
// string the caller wrote, so it is evaluated as-is: this script is a reviewer's tool
// pointed at our own page, not a sandbox for a stranger's URL.
//
// Zero dependencies: Node's built-in WebSocket, the same shape `browser-check.mjs`
// uses (and `host-probe/browser-check.mjs` before it).

import { spawn } from "node:child_process";
import { existsSync, writeFileSync } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";

const CHROME_CANDIDATES = [
  process.env.CHROME_PATH,
  "C:/Program Files/Google/Chrome/Application/chrome.exe",
  "C:/Program Files (x86)/Google/Chrome/Application/chrome.exe",
  "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
  "C:/Program Files/Microsoft/Edge/Application/msedge.exe",
  "/usr/bin/google-chrome",
  "/usr/bin/chromium",
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
];

/** `--flag value` pairs; every flag takes a value except `--help`. */
function parseArgs(argv) {
  const out = {};
  for (let at = 0; at < argv.length; at += 1) {
    const arg = argv[at];
    if (!arg.startsWith("--")) continue;
    const name = arg.slice(2);
    if (name === "help") {
      out.help = true;
      continue;
    }
    const value = argv[at + 1];
    if (value === undefined || value.startsWith("--")) {
      throw new Error(`${arg} needs a value`);
    }
    out[name] = value;
    at += 1;
  }
  return out;
}

function usage() {
  return [
    "node scripts/shot.mjs --url <url> [--viewport WxH] [--out file.png] [--eval <js>] [--wait ms]",
    "",
    "  --url       page to open (required)",
    "  --viewport  1440x900 by default",
    "  --out       screenshot path; omitted, no screenshot is taken",
    "  --eval      expression to run after load, printed as JSON",
    "  --wait      ms to wait after load before measuring; 6000 by default",
  ].join("\n");
}

const args = parseArgs(process.argv.slice(2));
if (args.help) {
  console.log(usage());
  process.exit(0);
}
if (!args.url) {
  console.error(usage());
  process.exit(2);
}

const [width, height] = (args.viewport ?? "1440x900").split("x").map((part) => Number(part));
if (!(width > 0) || !(height > 0)) throw new Error(`bad --viewport: ${args.viewport}`);

const chromePath = CHROME_CANDIDATES.find((candidate) => candidate && existsSync(candidate));
if (!chromePath) throw new Error("no Chrome or Edge found; set CHROME_PATH");

const port = 9200 + Math.floor(Math.random() * 300);
const chrome = spawn(
  chromePath,
  [
    "--headless=new",
    "--disable-gpu",
    "--no-first-run",
    "--no-default-browser-check",
    "--hide-scrollbars",
    `--remote-debugging-port=${port}`,
    `--user-data-dir=${process.env.TEMP ?? "/tmp"}/shot-profile-${port}`,
    "about:blank",
  ],
  { stdio: "ignore" },
);

/** The first page target, once Chrome has opened its debugging port. */
async function firstTarget() {
  for (let attempt = 0; attempt < 80; attempt += 1) {
    await sleep(250);
    try {
      const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      const page = list.find((target) => target.type === "page");
      if (page) return page;
    } catch {
      // Chrome is not listening yet; keep waiting.
    }
  }
  throw new Error(`Chrome never opened a debugging port on ${port}`);
}

const target = await firstTarget();
const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  ws.addEventListener("open", resolve, { once: true });
  ws.addEventListener("error", reject, { once: true });
});

let nextId = 0;
const pending = new Map();
ws.addEventListener("message", (event) => {
  let message;
  try {
    message = JSON.parse(event.data);
  } catch {
    return;
  }
  const resolve = pending.get(message.id);
  if (resolve) {
    pending.delete(message.id);
    resolve(message);
  }
});

/** One DevTools call, resolved with its result. */
function send(method, params = {}) {
  const id = (nextId += 1);
  return new Promise((resolve) => {
    pending.set(id, resolve);
    ws.send(JSON.stringify({ id, method, params }));
  });
}

try {
  await send("Page.enable");
  await send("Emulation.setDeviceMetricsOverride", {
    width,
    height,
    deviceScaleFactor: 1,
    mobile: width < 700,
  });
  await send("Page.navigate", { url: args.url });
  await sleep(Number(args.wait ?? 6000));

  if (args.eval) {
    const measured = await send("Runtime.evaluate", {
      expression: args.eval,
      returnByValue: true,
      awaitPromise: true,
    });
    const value = measured.result?.result?.value;
    console.log(typeof value === "string" ? value : JSON.stringify(value, null, 1));
  }

  if (args.out) {
    const shot = await send("Page.captureScreenshot", { format: "png" });
    if (!shot.result?.data) throw new Error("the screenshot came back empty");
    writeFileSync(args.out, Buffer.from(shot.result.data, "base64"));
    console.log(`shot: ${args.out}`);
  }
} finally {
  ws.close();
  chrome.kill();
}
