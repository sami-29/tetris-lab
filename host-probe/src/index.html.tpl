<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Host probe · ticket 09</title>
    <style>
      :root {
        color-scheme: dark;
        --bg: #0d1117;
        --panel: #161b22;
        --line: #30363d;
        --fg: #e6edf3;
        --dim: #8b949e;
        --ok: #3fb950;
        --bad: #f85149;
        --wip: #d29922;
      }
      * { box-sizing: border-box; }
      body {
        margin: 0;
        min-height: 100vh;
        display: grid;
        place-items: center;
        background: var(--bg);
        color: var(--fg);
        font: 14px/1.5 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
      }
      main { width: min(760px, 92vw); padding: 2rem 0; }
      h1 { font-size: 1.1rem; margin: 0 0 0.25rem; letter-spacing: 0.02em; }
      .sub { margin: 0 0 1.25rem; color: var(--dim); font-size: 0.85rem; }
      table {
        width: 100%;
        border-collapse: collapse;
        background: var(--panel);
        border: 1px solid var(--line);
        border-radius: 8px;
        overflow: hidden;
      }
      th,
      td { padding: 0.45rem 0.75rem; text-align: left; border-bottom: 1px solid var(--line); }
      tr:last-child th,
      tr:last-child td { border-bottom: 0; }
      th { width: 42%; color: var(--dim); font-weight: 500; }
      td { word-break: break-all; }
      .ok::before { content: "✓ "; color: var(--ok); }
      .bad::before { content: "✗ "; color: var(--bad); }
      .wip::before { content: "… "; color: var(--wip); }
      .verdict { margin-top: 1rem; padding: 0.75rem 0.9rem; border-radius: 8px; border: 1px solid var(--line); }
      .verdict.pass { border-color: var(--ok); color: var(--ok); }
      .verdict.fail { border-color: var(--bad); color: var(--bad); }
      .verdict.wait { border-color: var(--wip); color: var(--wip); }
      pre { margin: 1rem 0 0; color: var(--dim); white-space: pre-wrap; font-size: 0.8rem; }
      a { color: inherit; }
    </style>
  </head>
  <body>
    <main>
      <h1>Host probe</h1>
      <p class="sub">
        Ticket 09 fixture: static host → <code>.wasm</code> → module worker. Throwaway, not the Lab.
      </p>
      <table>
        <tbody>
          <tr><th>wasm path</th><td id="f-path">__WASM_PATH__</td></tr>
          <tr><th>fetch status</th><td id="f-status" class="wip">pending</td></tr>
          <tr><th>content-type</th><td id="f-ctype" class="wip">pending</td></tr>
          <tr><th>cache-control</th><td id="f-cache" class="wip">pending</td></tr>
          <tr><th>content-encoding</th><td id="f-encoding" class="wip">pending</td></tr>
          <tr><th>etag</th><td id="f-etag" class="wip">pending</td></tr>
          <tr><th>wasm bytes</th><td id="f-bytes" class="wip">pending</td></tr>
          <tr><th>abi_version()</th><td id="f-abi" class="wip">pending</td></tr>
          <tr><th>add(2, 40)</th><td id="f-sum" class="wip">pending</td></tr>
          <tr><th>probe_bits(5e6, 7)</th><td id="f-bits" class="wip">pending</td></tr>
          <tr><th>instantiate + compute</th><td id="f-timing" class="wip">pending</td></tr>
          <tr><th>crossOriginIsolated</th><td id="f-coi" class="wip">pending</td></tr>
        </tbody>
      </table>
      <div id="verdict" class="verdict wait">waiting on the worker…</div>
      <pre id="log"></pre>
    </main>
    <script type="module">
      const WASM_PATH = "__WASM_PATH__";
      const $ = (id) => document.getElementById(id);
      const set = (id, text, cls) => {
        const cell = $(id);
        cell.textContent = text;
        cell.className = cls;
      };
      const log = (...parts) => {
        $("log").textContent += parts.join(" ") + "\n";
      };

      $("f-coi").textContent = String(crossOriginIsolated);

      const verdict = $("verdict");
      const worker = new Worker("./worker.js", { type: "module" });
      const wasmUrl = new URL(WASM_PATH, location.href).href;

      worker.onmessage = ({ data }) => {
        if (!data.ok) {
          verdict.textContent = "worker failed: " + data.error;
          verdict.className = "verdict fail";
          log("worker error:", data.error);
          return;
        }
        const h = data.headers;
        set("f-status", String(h.status), h.status === 200 ? "ok" : "bad");
        const mimeOk = /^application\/wasm/.test(h.contentType);
        set("f-ctype", h.contentType || "(none)", mimeOk ? "ok" : "bad");
        const cacheOk = /immutable/.test(h.cacheControl) && /max-age=31536000/.test(h.cacheControl);
        set("f-cache", h.cacheControl || "(none)", cacheOk ? "ok" : "bad");
        set("f-encoding", h.contentEncoding || "(identity)", h.contentEncoding ? "ok" : "");
        set("f-etag", h.etag || "(none)", "");
        set("f-bytes", `${data.bytes} B (${(data.bytes / 1024).toFixed(1)} KiB)`, "");
        set("f-abi", String(data.abiVersion), data.abiVersion === 1 ? "ok" : "bad");
        set("f-sum", String(data.sum), data.sum === 42 ? "ok" : "bad");
        set("f-bits", String(data.bits), "");
        set(
          "f-timing",
          `${data.instantiateMs.toFixed(1)} ms instantiate · ${data.computeMs.toFixed(1)} ms compute`,
          "",
        );

        const pass = h.status === 200 && mimeOk && cacheOk && data.abiVersion === 1 && data.sum === 42;
        verdict.textContent = pass
          ? "host serves the module correctly: MIME, fingerprinted caching, and a worker running it"
          : "one or more checks failed; see the row marked ✗ and run verify.sh against the live URL";
        verdict.className = "verdict " + (pass ? "pass" : "fail");
        log(JSON.stringify({ url: data.url, wasmUrl }, null, 2));
      };
      worker.onerror = (e) => {
        verdict.textContent = "worker failed to start: " + (e.message || e.type);
        verdict.className = "verdict fail";
      };
      worker.postMessage({ url: wasmUrl });
    </script>
  </body>
</html>
