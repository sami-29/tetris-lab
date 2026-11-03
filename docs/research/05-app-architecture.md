# 05 — How the static app should be structured

Status: research, ticket 05. Input for ticket 12 (spec). Labels: **verified** = a primary source states it (link given); **inferred** = my reasoning from sources; **unverified** = not checked, listed under Open questions.

## Starting point (what the new UI replaces)

- The Flask app's page template (deleted at cutover; `git show 3c06a71:templates/index.html`) loaded htmx, Alpine and d3 from CDNs (lines 10–14), and posted forms to Flask routes `/run_simulation`, `/run_single_game` (lines 60, 123). **verified** (repo file, then).
- The app's `main.js` (same recovery: `git show 3c06a71:static/js/main.js`) parsed JSON responses from those routes and rendered stats; `updateInitialBoard` calls `fetch('/generate_initial_board')` (line 309). **verified** (repo file, then).
- The app's `tetris_visualization.js` (same recovery) drew the board as an SVG via d3, `cellSize = 30`, 20x10 grid, move animation driven by `animationInterval`/`isAnimating`. **verified** (repo file, then).
- Tabs live in the URL query (`?tab=`) with `pushState`. **verified** (main.js lines 18–35).
- Everything server-side disappears: the new app has no request/response boundary, only worker messages. Keep the URL-state and board-animation ideas; drop htmx/Alpine/d3.

## Worker-pool design

**Options**
- (A) N independent wasm instances, one per Web Worker, each with its own linear memory. No SharedArrayBuffer (SAB) needed for the batch itself.
- (B) One wasm module with threads: Emscripten `-pthread` + `PTHREAD_POOL_SIZE`, or Rust `wasm-bindgen-rayon`, on a shared memory.

**Evidence**
- Emscripten: threads need SAB, which browsers gate behind COOP/COEP; "pthreads code will not work in deployed environment unless these headers are correctly set." **verified** — https://emscripten.org/docs/porting/pthreads.html
- Emscripten: "not possible to build one binary" that uses threads when available and falls back; you need two builds and pick at runtime. **verified** — same page.
- Emscripten: `PTHREAD_POOL_SIZE` can be an expression such as `navigator.hardwareConcurrency`; pool workers are created before `main()`. **verified** — same page.
- Emscripten: `Atomics.wait` does not work on the main thread; blocking there busy-waits and can deadlock. Keep compute off the main thread. **verified** — same page.
- wasm-bindgen-rayon: Rust std thread support needs nightly, `build-std`, `+atomics,+bulk-memory`, `--shared-memory`, `--max-memory=1073741824`, `--target web`; pool started with `await initThreadPool(navigator.hardwareConcurrency)`. Its README demo shows a Mandelbrot at 273 ms single-thread vs 87 ms with the pool. **verified** — https://github.com/RReverser/wasm-bindgen-rayon (README; demo image alt text).
- A 5000-game batch is embarrassingly parallel: each game is independent and seeded. **inferred** from the domain (games do not share state).

**Verdict (inferred):** Option A for v1. Reasons: the batch needs no shared state; each instance keeps the single-threaded build (no nightly, no `build-std`, no two-build split); a worker that is terminated frees its memory cleanly, which helps cancellation. Option B only earns its place if a *single* solve must go faster, and that is ticket 03/06's question. Keep the door open: the core should not assume one instance.

**Pool sizing (inferred from sources):**
- `navigator.hardwareConcurrency` gives the number of logical processors, but "the browser may choose to report a lower number". **verified** — https://developer.mozilla.org/en-US/docs/Web/API/Navigator/hardwareConcurrency
- Proposed: `N = clamp(hardwareConcurrency - 1, 1, 8)`, leaving one core for the UI. Cap at 8 because each instance adds a linear memory and the pool gains little past the point where the UI and GC contend. The cap and the "-1" are judgments, not measured.
- Stockfish.wasm caps threads at 32 and warns that the platform may cap lower (`dom.workers.maxPerDomain` in Firefox). **verified** — https://github.com/lichess-org/stockfish.wasm (README "Current limitations"). Evidence that the pool must tolerate fewer workers than asked.
- Check `navigator.deviceMemory` before large per-instance allocations; Stockfish.wasm recommends it for its 1024 MB hash. **verified** — same README.

**Tab throttling**
- Chrome 88+: when a page is hidden, chained `setTimeout`/`setInterval` are throttled to once per second, and after 5 minutes hidden (chain count ≥5, silent 30 s) to once per minute. **verified** — https://developer.chrome.com/blog/timer-throttling-in-chrome-88
- The same article: `requestAnimationFrame` "will wait for the page to be visible, so it doesn't use any CPU when the page is hidden." **verified** — same source.
- Consequence: drive UI animation with rAF and push progress from workers via messages; never pace the batch with timers on the main thread. **inferred.**
- Whether a dedicated Worker is throttled while the tab is hidden is not stated in that article. **unverified** — see Open questions.

## Data across the JS/wasm boundary

**Facts that drive the choice**
- Structured clone copies data; `Function` objects cannot be cloned. **verified** — https://developer.mozilla.org/en-US/docs/Web/API/Web_Workers_API/Structured_clone_algorithm
- Transferable `ArrayBuffer`s move their memory in zero-copy; the sender's copy is detached (`byteLength` becomes 0). **verified** — https://developer.mozilla.org/en-US/docs/Web/API/Web_Workers_API/Transferable_objects
- `SharedArrayBuffer` is *not* transferable; it is shared, not moved. **verified** — https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/SharedArrayBuffer
- wasm-bindgen-rayon passes the `WebAssembly.Module` object to other threads. **verified** — its README "Why?" note.

**Sizing (inferred from the domain):**
- Board: 20x10 = 200 cells. As a `Uint8Array` that is 200 bytes; as bitboards it is 25 bytes. Copying this per message is free relative to a solve. No transfer needed.
- Move list for one solve: a few dozen placements, each encodable in 2 bytes (piece 3 bits, rotation 2, column 4). Tiny.
- Batch results: the per-game output is the risk. 5000 games × a few hundred bytes is ~1–2 MB if you ship full move lists. Ship per-game *summaries* (won, lines, attempts) as one typed array; ship full move lists only for the one game the visitor replays, on demand. **inferred** — the move-list-per-game size is my estimate, not measured.
- JSON: the current app sends JSON via htmx. For a numeric array, JSON costs both encode and parse; typed arrays avoid both. Magnitude **unverified** — no benchmark here; ticket 06 should measure.
- Progress: send small objects at ≤10 Hz, not per game. **inferred** (the Chrome throttling note shows the main thread cannot take unbounded wake-ups in the background).
- Shared memory for progress/cancel (an `Int32Array` on a SAB) avoids message traffic entirely, but requires the COOP/COEP headers on the host (ticket 02/09). Use it only if the host proves headers; otherwise messages. **inferred.**

## Cancellation of long jobs

A worker running wasm does not read its message queue until the wasm call returns. So a `cancel` message cannot interrupt a long call. Three layers, all needed:

1. **Chunk the work** so each wasm call is short (e.g. one game, or a fixed node budget per solve), then yield to the worker event loop between chunks. **inferred** from the Emscripten note that blocking calls need the event loop to be returned to.
2. **Cooperative flag**: check a cancel flag between chunks (a message-set boolean, or an `Int32Array` slot on a SAB if headers are proven).
3. **Hard kill**: `worker.terminate()` and re-create the worker from a cached `WebAssembly.Module`. Costs one instantiation per worker; the cost is **unverified** (ticket 06 should time it).

The orchestrator owns an `AbortController`-style token per job; the UI shows "stopped after X of 5000" from the last `progress` message, which the worker reports before acknowledging `cancelled`.

## wasm delivery

- Stream-compile: `WebAssembly.instantiateStreaming(fetch(url), imports)` compiles straight from the network bytes; the older `instantiate` needs the full ArrayBuffer first. **verified** — https://developer.mozilla.org/en-US/docs/WebAssembly/Guides/Loading_and_running
- Requirement that the response is served as `application/wasm`: **unverified** here (I did not fetch the MDN page for it). Ticket 09 must prove the MIME type on the live host.
- Import the wasm URL with Vite's `?url` suffix, and instantiate it inside the worker: **verified** — https://vite.dev/guide/features ("Manual Initialization", `?url`). Vite inlines `.wasm` below `build.assetsInlineLimit`; set it to `0` so the wasm stays a separate cached file. **verified** — same page (`assetsInlineLimit` and CSP note).
- Caching: content-hashed filenames with long `Cache-Control: max-age=31536000, immutable`; `index.html` revalidated. **inferred** practice; header syntax to be confirmed in ticket 09.
- Size budgets (proposed, **inferred**): wasm ≤ 150 KB brotli; entry JS ≤ 30 KB gzip; HTML+CSS first paint ≤ 50 KB; wasm fetched *after* first paint, in the worker, so the board renders before the engine loads. For scale: Stockfish.wasm's whole engine (JS + wasm + worker) is "~400K, ~150K gzipped". **verified** — https://github.com/lichess-org/stockfish.wasm.
- Multi-MB first paint is avoided by never putting wasm on the critical path of the HTML: the page is static, the worker loads the module asynchronously.

## Frontend stack

**Options:** (a) vanilla TypeScript + Vite; (b) a framework (React/Vue/Svelte/Solid); (c) no build at all, plain ES modules.

**Evidence**
- Vite: TypeScript is transpiled (Oxc, no type check); type-check with `tsc --noEmit`. `new Worker(new URL('./worker.ts', import.meta.url), { type: 'module' })` is the recommended worker form. **verified** — https://vite.dev/guide/features
- Vite: `?url` / `?worker` imports; `.d.wasm.ts` typings with `allowArbitraryExtensions`. **verified** — same page.
- ffmpeg.wasm ships the same wasm core behind a vanilla app and several framework apps (`apps/vanilla-app`, `apps/react-vite-app`, etc.), so the wasm itself does not dictate a framework. **verified** (repo tree) — https://github.com/ffmpegwasm/ffmpeg.wasm. The choice of framework is therefore a UI decision **inferred**.
- Comlink: ~1.1 kB brotli RPC over `postMessage`. **verified** — https://github.com/GoogleChromeLabs/comlink (README).

**Verdict (inferred):** (a) vanilla TS + Vite, hand-written typed messages (sketch below) rather than Comlink. Reasons: about six message types; typed protocol with explicit transfer lists is clearer than a proxy RPC that hides them; zero runtime dependencies. Comlink is a fallback if the message set grows.

**Board rendering (inferred):** Canvas 2D. A 20x10 board is 200 rects; redraw per rAF is trivial. WebGL/WebGPU would be overhead with no visible gain at this size. The Figma WebGPU article is about a large scene graph, not a 200-cell grid. **verified** (Figma blog, https://www.figma.com/blog/figma-rendering-powered-by-webgpu/, scope only). A DOM grid is a valid alternative; canvas keeps the animation and a 5000-game preview cheap.

**Dependency weight:** runtime deps 0; dev deps `vite` and `typescript` (+ optional `vitest`). Drops htmx, Alpine and d3 (three CDN scripts, all render-blocking today: index.html lines 10–14, the d3 one has no `defer`). **verified** (repo file).

## Architecture skeleton

```
web/                          # static site (Vite, TypeScript)
  index.html                  # static shell: board canvas, controls, no wasm
  public/_headers             # COOP/COEP + cache rules (ticket 09 proves them)
  src/
    main.ts                   # boot: feature-detect, mount UI, create Pool
    ui/
      board-canvas.ts         # draw 20x10 board, falling piece, move animation (rAF)
      controls.ts             # settings form, speed slider, play/pause/step
      stats.ts                # batch progress + result rendering
      url-state.ts            # ?tab= and settings in the query string (port of main.js)
    engine/
      pool.ts                 # N workers, queueing, cancel, backpressure
      job.ts                  # Job<T> handle: progress$, result, cancel()
      protocol.ts             # message types (shared by main and worker)
      wasm-url.ts             # `import url from '../../wasm/engine.wasm?url'`
    worker/
      engine.worker.ts        # instantiateStreaming, runs chunks, posts progress
  wasm/                       # build output of the native core (not hand-edited)
core/                         # native C++ (language per ticket 07; C++ leaning)
  include/tetris/engine.h     # pure API: solve(), play_game(), batch step
  src/                        # search, bitboards, RNG (seeded 7-bag)
  bindings/c_api.cpp          # extern "C" exports for the wasm build
  bench/                      # native benchmark binary (ticket 06), same core
  tests/                      # golden seeds, shared with the benchmark
```

**Where the native core sits:** `core/` is pure C++ with no browser or JS types. `bindings/` is the only place that knows about wasm (C ABI or embind). The same `core/` builds natively for `bench/` and tests, so the browser never has a second implementation. If ticket 07 picks Rust, the shape is the same with `core/` as a crate and `bindings/` as `wasm-bindgen`.

**Message shapes (sketch, TypeScript):**

```ts
// web/src/core/protocol.ts
export type JobId = number;

export type Board = Uint8Array;          // 200 cells, row-major, 0 = empty
export type PieceSeq = Uint8Array;       // piece ids 0..6, 7-bag order

export type JobSpec =
  | { kind: 'solve'; board: Board; pieces: PieceSeq; goal: number; beamWidth: number }
  | { kind: 'batch'; seedStart: number; games: number; goal: number; pieceCount: number; initialHeight: number };

export type ToWorker =
  | { type: 'init'; module: WebAssembly.Module; workerIndex: number }   // module from main
  | { type: 'run'; jobId: JobId; spec: JobSpec; chunk: number }
  | { type: 'cancel'; jobId: JobId };

export type FromWorker =
  | { type: 'ready'; workerIndex: number }
  | { type: 'progress'; jobId: JobId; done: number; total: number; won: number }
  | { type: 'moves'; jobId: JobId; moves: Uint16Array }                 // transfer moves.buffer
  | { type: 'done'; jobId: JobId; summary: Float64Array; moves?: Uint16Array }
  | { type: 'cancelled'; jobId: JobId; done: number }
  | { type: 'error'; jobId?: JobId; message: string };

export function transferList(msg: FromWorker): Transferable[] {
  return 'moves' in msg && msg.moves ? [msg.moves.buffer] : [];
}
```

Notes on the sketch:
- `module` is posted once per worker in `init`. Whether a `WebAssembly.Module` is structured-cloneable in all target browsers is **unverified** (wasm-bindgen-rayon relies on it for threads; check the JS API spec in ticket 06/09).
- `summary` is a fixed-layout `Float64Array` (won, games, sum of lines, attempts), not JSON.
- `progress` is throttled in the worker to ≤10 Hz.

**Pool (sketch):**

```ts
// web/src/core/pool.ts
export interface Job<T> { id: JobId; progress: ReadableStream<Progress>; result: Promise<T>; cancel(): void }
export interface Pool { size: number; submit<T>(spec: JobSpec): Job<T>; dispose(): void }
```

## Real deployed compute-heavy wasm apps

- **Stockfish.wasm (Lichess analysis)** — C++ engine via Emscripten, pthreads over SAB, so COOP/COEP are mandatory; feature-detects threads and falls back to a single-threaded port (stockfish.js). Caps: 32 threads, 1024 MB hash. **verified** — https://github.com/lichess-org/stockfish.wasm. Lesson: two builds + runtime detection is the real-world cost of option B.
- **ffmpeg.wasm** — pure wasm port of FFmpeg run from JS; ships vanilla, React, Vue, Angular, Svelte and Next examples, and a multithreaded transcode demo (`apps/vanilla-app/public/transcode-mt.html`). **verified** — https://github.com/ffmpegwasm/ffmpeg.wasm (repo tree). Lesson: the same core ships with and without threads; the wrapper is thin.
- **Figma** — C++ editor compiled with Emscripten; the 2017 post reports wasm cut load time about 3x. **verified** — https://www.figma.com/blog/webassembly-cut-figmas-load-time-by-3x/ (dated 2017-06-08). Lesson: wasm delivery/load time is the first thing to budget, not raw speed. The 2025 WebGPU renderer post is a separate rendering story.

## Recommendation

Ship option A (N single-thread wasm instances in workers, N = clamp(cores−1, 1, 8)), vanilla TS + Vite, Canvas 2D board, typed messages with transfer lists, chunked jobs with cooperative cancel plus `terminate()`, and the `core/` + `bindings/` split above. Upgrade to threads only if ticket 06 shows a single solve is latency-bound and cores are idle.

**Main reason:** the workload is independent seeded games, which needs no shared memory, so threads buy complexity (nightly or two builds, COOP/COEP as a hard dependency) for no gain on the batch path.

**What would change it:** (1) a measured single-solve latency that is too high with one core while cores sit idle → add a threaded build for solve only; (2) ticket 09 showing headers cannot be set → SAB unavailable, which strengthens A; (3) ticket 07 picking Rust → same skeleton, different `bindings/`.

## Open questions

- Whether Chrome/Firefox/Safari throttle a hidden tab's *dedicated* Workers (the Chrome article covers timers only). **unverified**
- `WebAssembly.Module` structured-clone support across target browsers. **unverified**
- MIME type requirement for `instantiateStreaming` (`application/wasm`). **unverified** here; ticket 09.
- Cost of per-game move-list output vs summary output for 5000 games; JSON vs typed-array cost. **unverified**, needs ticket 06 numbers.
- Re-instantiation cost after `terminate()`. **unverified**
- Concrete `_headers` syntax and whether the chosen host compresses `.wasm` (ticket 02/09). **unverified**
- Size budgets above are proposals, not measured against a real build. **inferred**
- Canvas 2D vs DOM grid at 200 cells: no benchmark. **inferred**
