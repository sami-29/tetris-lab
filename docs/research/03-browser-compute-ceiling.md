# 03 — How fast can the browser actually go

Status: research complete (desk research; no benchmarks run on this machine).
Ticket: the build ticket for this note (a local tracker; not part of the repository).

Labels: **verified** = a primary or first-party source states it. **inferred** = reasoned from sources.
**unverified** = not confirmed by a source I read. Numbers marked *ROUGH* are my estimates, not measurements.

## 0. Workload shape (from the repo)

- `TetrisSolver.py`: progressive beam search, schedule `(1, 3, 7, 25)`. For each beam state, each of the piece's rotations and each valid column is one "evaluation" (`evaluations += 1`, line ~151). Each one does `drop_row`, a board copy, `place_piece`, `clear_lines`, then `engine.evaluate` (line ~165-170). So one counted evaluation = one placement simulation plus one heuristic score.
- `tetris_engine.py`: 20x10 `uint8` numpy board; `evaluate` = column heights (argmax), holes (cumsum), bumpiness, line count. All are NumPy calls, so the current hot path is CPython plus NumPy call overhead, not arithmetic.
- Scale: the ticket's 600–70,000 evaluations per game is consistent with width ~25 x ~35 candidates per step x ~40 pieces at the widest pass (inferred from the code; not re-measured).
- Data size: 200 cells = 200 bits. A bitboard is 4 x u64 or 20 x u16 rows (or 10 x u32 columns). The whole board lives in registers/L1. Memory bandwidth is not the bottleneck; branching and per-candidate overhead are.

## 1. Options

- **A. Scalar wasm, bitboard rewrite** (C++/Rust/etc. compiled to wasm32, no SIMD).
- **B. Wasm SIMD128** on top of A (v128 for row/column packed ops).
- **C. Wasm threads** (SharedArrayBuffer + Worker pool) running independent games.
- **D. WebGPU compute** (from wasm via Emscripten/emdawnwebgpu or wgpu), for one solve or for batches.
- Orthogonal: **memory64** (is it needed?).

## 2. Criteria

1. Latency for one interactive solve (target: well under 100 ms; budget ~0.1 s).
2. Throughput for batch winnability stats (thousands of games).
3. Dispatch/sync overhead relative to per-step work.
4. Deployment risk: headers, browser support, host constraints (static hosting).
5. Engineering cost and portability.

## 3. Findings per topic

### (a) WASM SIMD128

- Support: SIMD128 is shipped in Chrome 91, Firefox 89, Safari 16.4, Node 16.4+. **verified** (Emscripten SIMD docs: https://emscripten.org/docs/porting/simd.html ; caniuse global usage ~96.5%: https://caniuse.com/wasm-simd).
- Ops available for bitboard code (SIMD spec, `proposals/simd/SIMD.md`): v128 `and/or/xor/not/andnot/bitselect`; lane shifts `shl/shr_s/shr_u`; `i8x16.popcnt` (only 8-bit lanes; no i64x2 popcnt in the spec); `bitmask` (i8x16/i16x8/i32x4/i64x2 -> scalar mask); `any_true`/`all_true`; `shuffle` and `swizzle`; `min/max/avgr_u`; `extmul`/`extadd_pairwise`. **verified** (https://github.com/WebAssembly/simd/blob/main/proposals/simd/SIMD.md). No count-leading/trailing-zero vector op is listed in the core SIMD set. **verified** (absence in the same spec).
- Emscripten's own table of SSE-to-wasm mappings says `_mm_movemask_ps` maps to `wasm_i32x4_bitmask` and that per-lane shifts with non-constant amounts and `i8x16.swizzle` with variable indices cost extra instructions on x86 engines. **verified** (https://emscripten.org/docs/porting/simd.html, performance table).
- Which ops compilers emit: LLVM autovectorises loops over `u16`/`u8` lanes and emits the bitwise and shift ops above; scalar `i32/i64.popcnt` and `clz/ctz` remain scalar instructions. I did not verify exact codegen by reading LLVM output. **unverified** (codegen claim); the op list is verified.
- Relaxed SIMD: Chrome 114+, Firefox 146+, **not** Safari. **verified** (https://caniuse.com/wf-wasm-simd-relaxed ; https://webstatus.dev/features/wasm-simd-relaxed). Not needed here; avoid it.
- Speedup: no primary source gives a SIMD-vs-scalar number for a branchy, 200-bit board search. The only numbers I found are a 2019 Wasmer post ("up to 16x on 8-bit ops") and generic microbenchmarks. Those are upper bounds for pure streaming loops, not this workload. **unverified** for this workload.
- My estimate: for this shape, SIMD buys ~1.0–1.5x over a well-written scalar bitboard, because the work per candidate is a handful of 64-bit ops plus branches, and the 200-bit board already fits in 4 u64 words. *ROUGH, inferred.* Algorithmic bitboard form (A) is worth far more than SIMD (B). The sizable win is replacing numpy calls, not the vector unit.

### (b) WASM threads (SharedArrayBuffer)

- Threads need `SharedArrayBuffer`, which browsers gate behind cross-origin isolation (COOP + COEP). Emscripten pthreads "will not work in deployed environment unless these headers are correctly set." **verified** (https://emscripten.org/docs/porting/pthreads). Chrome desktop moved SAB behind COOP/COEP around M91 (Emscripten issue #12472; a secondary-source quote, treat as **inferred** for exact version).
- Headers: `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp`. This is what unlocks `SharedArrayBuffer`, `WebAssembly.Memory` backed by SAB, and precise timers. **verified** (https://web.dev/articles/webassembly-threads ; MDN COEP page https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Cross-Origin-Embedder-Policy).
- Static hosts without header control: `coi-serviceworker` injects the headers via a service worker, but it reloads on first visit. Its COEP `credentialless` mode is not supported on Safari, and there is a reported Firefox breakage on refresh (issues #20, #23). **verified** (https://github.com/gzuidhof/coi-serviceworker and its issues). Implication: the host choice (ticket 02/09) must support headers, or threads are off by default and the app must degrade to single-thread.
- Pool sizing: `navigator.hardwareConcurrency` returns logical cores; web.dev recommends passing it to a pool initializer "if you want to use all cores", and Emscripten has `-sPTHREAD_POOL_SIZE`. Pre-create the pool before use (pthread_create is async; deadlock otherwise). **verified** (https://web.dev/articles/webassembly-threads).
- Scaling: independent games need no shared mutable state, so scaling is ~linear up to physical cores. Above that, SMT and E-cores give diminishing returns. *ROUGH, inferred.* Recommended: `workers = clamp(hardwareConcurrency - 1, 1, 8)` (leave one for UI; cap because mobile and throttled tabs gain little). Unverified: real scaling curves in browsers.
- Support caveats: SAB and threads exist in all major engines once isolated; Safari and mobile are the frequent failure points. Safari threads support was not verified by me. **unverified**.
- Memory is per worker: each worker shares the same SAB heap if using pthreads, or has its own instance if using separate instances. Both cost memory; neither is a problem at this workload size (see (c)).

### (c) Memory limits and memory64

- wasm32 linear memory: max 4 GiB (65536 pages). Initial memory is capped at 32767 pages (~2 GiB); the maximum was raised to 4 GiB. **verified** (https://v8.dev/blog/4gb-wasm-memory ; https://github.com/WebAssembly/spec/pull/1121).
- memory64: shipped Chrome 133 (Feb 2025), Firefox 134 (Jan 2025). Safari: not supported (as of the caniuse snapshot). **verified** (https://web-platform-dx.github.io/web-features-explorer/features/wasm-memory64/ ; https://caniuse.com/wf-wasm-memory64 ; Chrome intent: https://groups.google.com/a/chromium.org/g/blink-dev/c/5vTbd1dttwc/m/Z4UFehJBAgAJ).
- Decision for this workload: **not needed**. Beam state for width 25 is ~25 x 200 bits plus parent-pointer nodes; even 70,000 candidate records at tens of bytes is a few MB. Memory64 is also not Baseline (Safari gap), so it is a liability.
- Allocation cost: avoid per-candidate allocation in wasm. Preallocate a candidate arena per step and use u64 boards. The current Python code allocates a board copy per candidate (`state.board.copy()`); that is the thing to remove. **inferred** from TetrisSolver.py.

### (d) WebGPU compute

- Toolchain: Emscripten's `-sUSE_WEBGPU` is deprecated in favour of the `emdawnwebgpu` port (`--use-port=emdawnwebgpu`). **verified** (https://emscripten.org/docs/porting/multimedia_and_graphics/WebGPU-support.html ; PR #24220 https://github.com/emscripten-core/emscripten/pull/24220 ; https://github.com/emscripten-core/emscripten/blob/main/tools/ports/emdawnwebgpu.py). wgpu (Rust) also targets the browser WebGPU API from wasm32. **inferred/known**, not re-verified here.
- Limits: default `maxBufferSize` 256 MiB; default `maxStorageBufferBindingSize` 128 MiB. **verified** (https://gpuweb.github.io/cts/docs/tsdoc/functions/webgpu_capability_info.getDefaultLimits.html ; https://github.com/webgpu/webgpufundamentals/blob/87b9dd3b/webgpu/lessons/webgpu-storage-buffers.md). Not a constraint at this size.
- **Dispatch latency (primary data, arXiv 2604.02344, Table 6):**
  - Sequential dispatch cost (sync only at end): Chrome/Linux Vulkan 32.8 µs; Chrome/Win D3D12 58.7–66.5 µs; Safari/Metal 31.7 µs; **Firefox ~1,037–1,047 µs** (rate-limited, mechanism not confirmed by the authors).
  - Single-op including GPU->CPU sync: Chrome 2,071–3,124 µs; Safari 248 µs; Firefox ~103,000 µs. So a readback round trip is ~1–3 ms in Chrome, ~0.2 ms in Safari, ~100 ms in Firefox. **verified** (the paper's own measurements; preprint, single author, browser versions Chrome 144 / Safari 26.2 / Firefox 147).
  - Corroboration: Mozilla bug 1900273 reports `mapAsync` READ taking ~100 ms for 3 floats in Firefox 128 on Linux (user report, not a benchmark). **verified as a report.**
- **Bandwidth:** I found no primary browser-measured GB/s figure. Public benchmark pages (SystemCheck, greggman/webgpu-benchmark) exist but I did not read numbers from them. **unverified**; a rough PCIe-class figure (~10–25 GB/s host<->device) is a general estimate, *ROUGH*, not needed here because the payload per step is bytes, not MB.
- Verdict per use case:
  - **(a) One interactive solve:** rejected. A step with width 25 is ~25 x 35 ≈ 875 candidates. On CPU at ~10 M candidates/s (ROUGH, see budget) that step is ~90 µs. A single GPU dispatch in Chrome costs ~33–66 µs of API time, and a readback sync adds ~1–3 ms. A solve needs ~40 sequential steps (one per piece), each with a dependency on the previous beam, so the GPU path costs ~40 x (dispatch + sync) ≈ 40–120 ms in Chrome and ~40 s in Firefox before any compute. **inferred** from the verified numbers above. The CPU path finishes a full solve in single-digit milliseconds (ROUGH).
  - **(b) Thousands of games:** plausible but not justified yet. Games are independent, so the GPU could run many games in parallel, one per invocation, with the whole beam search in one shader. The cost is a hard rewrite: per-game divergent control flow, in-shader beam sort, 40 sequential steps (can be one dispatch if the whole game runs in-shader). The dispatch-overhead problem disappears in that design, but the work is large and the result is unmeasured. Keep it as a Phase-2 option gated on a measured CPU-worker baseline (ticket 06). **inferred.**

## 4. Budget

Assumptions (all ROUGH unless stated):
- Candidate ("evaluation") = drop + place + clear + evaluate. Count per game from the ticket: 600–70,000.
- CPython + NumPy today: ~20–50 µs per candidate (many small NumPy calls) -> ~20–50 k/s per core. *ROUGH; not measured on this machine (no runs).*
- Wasm scalar bitboard: ~50–150 ns per candidate -> **~7–20 M/s per core**. *ROUGH, inferred from op counts; to be measured in ticket 06.*
- Wasm SIMD128 on top of scalar bitboard: x1.0–1.5. *ROUGH.*
- Worker scaling: 0.7–0.9 efficiency up to physical cores; SMT adds less. *ROUGH.*

| Quantity | Python today (ROUGH) | Wasm scalar bitboard (ROUGH) | + SIMD (ROUGH) |
|---|---|---|---|
| evals/s per core | 20–50 k | 7–20 M | 7–30 M |
| one solve, 600 evals | <1 ms | 30–90 µs | 20–90 µs |
| one solve, 70,000 evals | 1.4–3.5 s | 3.5–10 ms | 2.5–10 ms |
| 10,000 games x ~10k evals (1e8 evals), 1 core | ~1 h | 5–14 s | 4–12 s |
| same, 8 workers (eff 0.8) | ~8 min | 0.6–1.8 s | 0.5–1.5 s |

Reading:
- One interactive solve is fast on one wasm core. The threads and GPU budgets are not needed for latency.
- Batch winnability runs are where workers pay off (~6–7x on 8 workers at 0.8 efficiency). They also make the measured CPU baseline the thing to beat before any GPU work.

Accelerator placement:
- **WASM scalar bitboard:** belongs in the solver (primary). Evidence: sizes fit in L1, branch-heavy, sub-ms per solve.
- **WASM SIMD128:** optional micro-optimisation; add after the scalar bitboard is measured. Evidence: op set verified; gain unverified.
- **Threads (SAB workers):** belong in the batch/lab path only, with a single-thread fallback when the page is not cross-origin isolated. Evidence: COOP/COEP requirement and host constraint.
- **memory64:** rejected. Evidence: not needed (4 GiB cap, ~MB working set) and not Baseline (Safari).
- **WebGPU:** rejected for the interactive solve (dispatch + sync exceed whole-solve compute). Deferred for batches (Phase 2, needs in-shader game loop and a measured CPU baseline). Evidence: Table 6 numbers and the WebGPU limits.

## 5. Recommendation

Design the solver as scalar wasm bitboard, run batches on a worker pool gated by cross-origin isolation, skip memory64, and do not build WebGPU into v1.

Main reason: the workload is 10^3–10^5 tiny, sequential, branchy steps on a board that fits in registers. Dispatch and readback overhead in browsers (32 µs to ~1 ms per dispatch, plus 1–3 ms sync in Chrome and ~100 ms in Firefox) is larger than the whole solve's arithmetic.

What would change it: (1) a measured CPU baseline below ~1 M candidates/s per core (then reconsider GPU for batches); (2) a batch target of 10^7+ games where per-game in-shader search is feasible; (3) browser-measured WebGPU readback in Chrome/Firefox dropping to ~10 µs; (4) the host being unable to set COOP/COEP, which would make threads unavailable for the batch path.

## 6. Open questions (unverified)

1. Real wasm scalar bitboard candidates/s per core in Chrome/Firefox/Safari (needs ticket 06 benchmark).
2. Actual SIMD128 speedup for this bitboard op mix (codegen not inspected).
3. Browser worker-pool scaling curves on SMT/E-core laptops; whether browsers under-report `hardwareConcurrency`.
4. Safari threads/SAB support details and mobile behaviour.
5. Measured browser WebGPU host<->device bandwidth (GB/s). Not found in a primary source.
6. Whether a full beam solve can be expressed as one WebGPU dispatch per game without workgroup-storage limits (default limits not verified here).
7. LLVM codegen for `popcnt`/`clz` on wasm32 scalar and vector paths.
8. Whether `coi-serviceworker` behaviour in current Firefox/Safari has been fixed since issues #20/#23.

## Sources

- SIMD spec: https://github.com/WebAssembly/simd/blob/main/proposals/simd/SIMD.md
- Emscripten SIMD: https://emscripten.org/docs/porting/simd.html
- caniuse wasm-simd: https://caniuse.com/wasm-simd ; relaxed: https://caniuse.com/wf-wasm-simd-relaxed
- Emscripten pthreads: https://emscripten.org/docs/porting/pthreads
- web.dev threads: https://web.dev/articles/webassembly-threads
- MDN COEP: https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Cross-Origin-Embedder-Policy
- coi-serviceworker: https://github.com/gzuidhof/coi-serviceworker (issues #20, #23)
- V8 4 GiB: https://v8.dev/blog/4gb-wasm-memory ; spec PR 1121: https://github.com/WebAssembly/spec/pull/1121
- memory64 status: https://web-platform-dx.github.io/web-features-explorer/features/wasm-memory64/ ; https://caniuse.com/wf-wasm-memory64
- WebGPU limits: https://gpuweb.github.io/cts/docs/tsdoc/functions/webgpu_capability_info.getDefaultLimits.html ; https://github.com/webgpu/webgpufundamentals/blob/87b9dd3b/webgpu/lessons/webgpu-storage-buffers.md
- Emscripten WebGPU: https://emscripten.org/docs/porting/multimedia_and_graphics/WebGPU-support.html
- WebGPU dispatch overhead (arXiv 2604.02344, Table 6): https://arxiv.org/html/2604.02344
- Firefox mapAsync report: https://bugzilla.mozilla.org/show_bug.cgi?id=1900273
- Wasm SIMD 2019 speedup claim (weak, upper bound only): https://blog.wasmer.io/webassembly-and-simd-13badb9bf1a8
- Repo: `TetrisSolver.py` (evaluation counting, lines ~129-179), `tetris_engine.py` (`evaluate`, numpy path).
