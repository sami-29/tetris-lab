# 01 — Language and WASM toolchain (research)

**Question.** For a CPU-bound 20×10 bitboard beam-search workload plus batches of thousands of seeded games, which wins: C++/Emscripten, Rust (wasm-bindgen, or raw `wasm32-unknown-unknown` + JS glue), Zig, or other?

**Recommendation.** **Rust stable, `wasm32-unknown-unknown`, `core::arch::wasm32` SIMD128, and a Web Worker pool where each worker owns its own wasm instance (no shared memory).** Glue choice (wasm-bindgen vs raw) is deferred to the 06 spike: both score the same here. Main reason: measured performance does not separate the options (same LLVM backend, same SIMD gap), so the decision rests on build/test cost and the thread requirement, and Rust wins both. Confidence: moderate.

---

## 1. Options

| ID | Option | Setup assumed |
|---|---|---|
| A | C++ / Emscripten | `emcc -O3 -msimd128 [-pthread -sPTHREAD_POOL_SIZE=N]`, `wasm_simd128.h` or GCC vector extensions, embind or raw exports |
| B | Rust / wasm-bindgen | `wasm32-unknown-unknown`, `-Ctarget-feature=+simd128`, `--target web`, `wasm-opt -O3` |
| B′ | Rust / raw glue | Same target, no wasm-bindgen; hand-written `WebAssembly.instantiate` + typed-array glue |
| C | Zig | `zig build-lib -target wasm32-freestanding -O ReleaseSmall`, `@Vector` |
| D | Other (AssemblyScript, hand WAT) | Not researched; see §9 |

## 2. Criteria

1 SIMD128 access and codegen. 2 Threads (batches; intra-game). 3 Native→wasm ratio evidence. 4 Binary size and startup. 5 Build/iteration cost **on this machine**. 6 JS-boundary ergonomics (boards, move lists). 7 Debuggability and host testability. 8 Long-term maintenance for a portfolio repo.

## 3. Local facts (run this session)

| Probe | Result | Status |
|---|---|---|
| `g++ --version` | g++ 13.2.0 (MinGW-builds, posix-seh) | verified (run) |
| `cargo --version` | **error**: rustup "could not choose a version of cargo… no default is configured" | verified (run) |
| `rustup toolchain list` | "no installed toolchains". `cargo.exe`/`rustc.exe` shims exist in `~/.cargo/bin`, nothing behind them | verified (run) |
| `node --version` / `python --version` | v24.21.0 (V8 13.6) / 3.11.9 | verified (run) |
| emcc, cmake, clang, zig, docker, wasm-bindgen, wasm-opt, wasm-pack | not on PATH | verified (run) |
| Node SIMD128 | Hand-assembled module using `v128.const` + `i32x4.extract_lane` validates and returns 7 | verified (run, /tmp probe) |
| Node shared memory | `new WebAssembly.Memory({shared:true})` yields a `SharedArrayBuffer` | verified (run) |

Consequences: the C++ core can be compiled and unit-tested **today** with native g++ (not wasm). Rust needs a network-dependent `rustup default stable` and `rustup target add wasm32-unknown-unknown` before anything builds. Emscripten needs an emsdk install. I did not attempt either install, as instructed. Node can run SIMD and shared-memory wasm, so the browser-runtime side has no blocker. No compiler for A, B, or C was run.

## 4. Current implementation: what is ported, and its speed

Read: `TetrisSolver.py` (193 lines), `tetris_engine.py` (130), `tetromino_data.py` (37), `TetrisGameGenerator.py` (133), `app.py` defaults.

- Search: progressive beam, schedule `(1, 3, 7, 25)`. Per state: rotations × valid columns → `drop_row` → copy board → `place_piece` → `clear_lines` (`np.vstack`) → `evaluate` (numpy). Sort and trim to width. Returns on the first width reaching `goal` lines.
- Heuristic: `1000·lines − 51·Σheights − 35·holes − 18·bumpiness`.
- Generator: `random.Random(seed)` fills the board to `initial_height_max=7`; 7-bag sequence with S/Z reshuffle.
- App defaults: goal 8, 40 pieces, height 7. `app.py` batches via `multiprocessing`.

**Measured here (CPython 3.11, numpy, one core, seeds 0–19, `PYTHONDONTWRITEBYTECODE=1`):**

| Measure | Result |
|---|---|
| `engine.evaluate(board)` alone | ~16.7k calls/s (≈60 µs/call) |
| Solver, goal 8 (app default), 40 pieces | 20/20 solved; 11,273 candidate evals in 2.06 s → **≈5.5k evals/s**; mean **≈103 ms/game ≈ 10 games/s** |
| Solver, goal 15 (generator `main()` default) | 20/20 solved; 66,598 evals in 17.7 s → **≈3.8k evals/s**; ≈1.1 games/s |

"Eval" = one candidate placement scored (`evaluations += 1`). Order of magnitude: **10³ evals/s, ~10 games/s per core.**

Reading: `evaluate()` takes ~60 µs on a 20×10 array. That is interpreter and numpy-call overhead, not arithmetic. Most of any speedup comes from leaving Python, not from the wasm-vs-native ratio. Check any claimed speedup against 3.8k–5.5k evals/s and ~10 games/s/core.

## 5. Evidence on performance

### 5a. Native→wasm ratios (measured by others)

**Jangda et al., "Not So Fast", ATC'19 (arXiv 1901.09056). Verified** from Table 1 and §4.2. Clang 4.0 native vs Emscripten wasm; Chrome 74, Firefox 66; 2019; no SIMD.
- SPEC CPU geomean slowdown **1.55× Chrome, 1.45× Firefox**; median 1.53× / 1.54×.
- Integer, search-like rows (ratios computed from Table 1):
  - 458.sjeng (chess game-tree search): 358 → 602 ms (**1.68×**) Chrome; 580 (**1.62×**) Firefox.
  - 445.gobmk (Go search): 352 → 537 (**1.53×**) / 549 (**1.56×**).
  - 462.libquantum (bit ops): 330 → 444 (**1.35×**) / 385 (**1.17×**).
  - 429.mcf (pointer chasing): 221 → 180 (**0.81×**) / 184 (0.83×).
  - 401.bzip2: 370 → 864 (**2.34×**) / 730 (1.97×).
- Causes (verified, §1, §4): about 2× the loads and stores of native (fewer registers, weaker allocator); extra branches from dynamic safety checks; larger code footprint. Their build used `-fno-strict-aliasing` (footnote 4).
- Scope limit: 2019 engines, no SIMD measured. Read as an upper bound for scalar integer code on those engines.

**BLAKE3 PR #341 (Rust integer SIMD hashing). Verified** from the PR body. Run on wasmtime v12.0.1, **not a browser**.
- x64: native intrinsics 7,979 ns vs wasm SIMD128 11,985 ns → **1.50×**. Native portable 6,713 ns vs wasm scalar 25,978 ns → **3.87×**.
- Apple M1: native 11,610 ns vs wasm SIMD 13,355 ns → **1.15×**. Wasm scalar 22,037 ns → **1.90×**.
- Maintainer's bench (comment 2025-03-17): wasm scalar 273 MB/s vs wasm SIMD 1,640 MB/s → **6.0×** SIMD gain *within* wasm.
- Reading: with SIMD128 the wasm/native gap for integer-vector code is ~1.15–1.5×; without it, 2–4×. That is the strongest argument for *using SIMD*. It is not evidence for one language over another.

**Inferred band:** bitboard code on wasm with SIMD128 lands near 1.2–1.7× native. Scalar u64 bit ops map to plain i64 ops, so the scalar penalty may be smaller than the hashing numbers. Not measured here; ticket 06 must measure.

### 5b. SIMD128 codegen per toolchain

- **Emscripten (A). Verified.** `-msimd128` enables `wasm_simd128.h` intrinsics, GCC/Clang vector extensions, and autovectorization; `-fno-vectorize -fno-slp-vectorize` disables the vectorizer. Browser support: Chrome ≥91, Firefox ≥89, Safari ≥16.4, Node ≥16.4. Caveat (verified): several SSE ops are emulated (general shuffles, `i8x16.mul`, `i64x2.shr_s`, saturating float→int). Bitboard code mostly needs `and/or/xor/shl/shr/popcnt`, which map directly, but check each op used.
- **Rust (B/B′). Verified.** `core::arch::wasm32` exposes `simd128` intrinsics (stable) under `#[target_feature(enable="simd128")]` or `-Ctarget-feature=+simd128`. Doc: the std library is not SIMD-compiled unless rebuilt; no runtime detection, so the binary either has SIMD or not. `relaxed-simd` is a separate gated feature. Same LLVM backend as Emscripten, so emitted SIMD should be comparable (**inferred**, not diffed).
- **Zig (C). Verified only that** `@Vector` "will typically compile to single SIMD instructions" (Zig docs, read via summary fetch). Wasm simd128 lowering **unverified**.

### 5c. Threads

- **Emscripten. Verified** (pthreads doc): `-pthread` at compile and link; `-sPTHREAD_POOL_SIZE` pre-spawns workers; `PROXY_TO_PTHREAD`; `Atomics.wait` cannot block the main thread; mimalloc for malloc contention. Doc says "stable". **Doc inconsistency:** one paragraph still says pthread builds "will currently only work in the Firefox Nightly channel". Treat as stale; the top section and MDN gating are current.
- **Rust. Verified** (wasm32-unknown-unknown page): `std::thread::spawn` **panics** on this target; the target is not tested in rust-lang CI. `wasm-bindgen-rayon` README: needs a pinned nightly (`nightly-2025-11-15` tested), `-Ctarget-feature=+atomics,+bulk-memory`, `build-std`, `--shared-memory`, and COOP/COEP.
- **Zig. Verified** from `lib/std/Thread.zig` on master: `Impl` is pthreads / Windows / Linux / WASI / else `UnsupportedImpl`, which is a `@compileError`. So `std.Thread.spawn` **does not build for wasm32-freestanding** (the browser target). Threads would need hand-rolled Worker glue.
- **Browser gate. Verified** (MDN): `SharedArrayBuffer` needs `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp|credentialless`. Without them, `crossOriginIsolated` is false and the page falls back to `ArrayBuffer`. `postMessage` of an `ArrayBuffer` works either way.
- **Design point (inferred from the MDN fallback):** batches of independent seeded games need no shared memory. A pool of Workers, each with its own `WebAssembly.Instance`, receiving seeds and returning results by `postMessage`, is enough. That removes the COOP/COEP dependency for the batch path and makes the nightly question moot. Shared memory is only needed for intra-game parallelism, which the spec does not require. Ticket 09 still confirms headers either way.

### 5d. Binary size and startup

- Rust. Verified guidance (Rust and WebAssembly book): `lto = true`; `opt-level='s'|'z'`; `wasm-opt -Oz` saves ~15–20%; panics become aborts on wasm32-unknown-unknown; default dlmalloc ~10 KB; `format!` and panic paths bloat. Expected total for this workload: tens of KB (**inferred**, not built).
- Emscripten. JS runtime glue ships alongside the `.wasm`. Size **unverified** (not built). `MINIMAL_RUNTIME` and `-Oz` exist; not checked against docs here.
- Startup. Browsers compile wasm with a baseline tier while it streams (Rust book, verified). Module size matters less than first-call compile cost. **Not measured.**

### 5e. Build and iteration cost on this machine

| Option | Present now | Must install (not done) | Native test loop now |
|---|---|---|---|
| A C++/Emscripten | g++ 13.2 (native), node | emsdk (size and time **unverified**) | **Yes**: g++ builds and tests the core natively (x86 path, not the wasm SIMD path) |
| B/B′ Rust | cargo/rustc shims, no toolchain | `rustup default stable` + `rustup target add wasm32-unknown-unknown` (network; size **unmeasured**) | After install: `cargo test` on host, same source |
| C Zig | nothing | Zig binary (**unmeasured**) | After install |

Ranking here: C++ core testable immediately; Rust one `rustup` command from a wasm build; Zig worst on threads and not installed. Both Emscripten and Rust need network installs; neither was run.

### 5f. JS-boundary ergonomics

Workload data is small and numeric: board = 20 rows × u16 = 40 bytes; move = (piece, rot, col) packed into u16/i32; batch results = a flat typed array. Recommended boundary for either: one call per batch, `Int32Array`/`Uint32Array` in and out, no per-game calls.
- Emscripten: `cwrap`/`ccall` or embind. Raw HEAP views are fast but break when memory grows (pthreads doc warns about memory-growth views).
- Rust: wasm-bindgen generates typed-array glue; slices and `Vec` cross as copies. Raw glue (B′) is short for a handful of exports and avoids the HEAP-view trap.

### 5g. Debuggability, testability, maintenance

- Testability: Rust `cargo test` runs identical source natively and under wasm (wasmtime or node runner). C++ `g++` tests the core natively, but the wasm path needs emsdk to test.
- Browser debugging: both emit DWARF or name sections. Chrome DevTools support quality **not verified** (the Chrome blog URL returned 404; search was rate-limited).
- Maintenance: Rust is memory-safe by default. The Jangda build needed `-fno-strict-aliasing`, a hint of aliasing risk in C/C++ ports. Zig master churn is visible (`std.Io` inside `Thread.zig`), so pin versions. Rust nightly is only needed for wasm threads, which the batch design avoids.

## 6. Scoring (1 = poor, 5 = strong; judgement from §3–§5)

| Criterion | A C++/Emscripten | B Rust/wasm-bindgen | B′ Rust/raw | C Zig | Basis |
|---|---|---|---|---|---|
| 1 SIMD128 access | 5 | 5 | 5 | 4 | A, B verified; C unverified |
| 2 Threads | 4 (stable, needs COOP/COEP) | 3 (nightly for threads; not needed for batches) | 3 | 1 (no `std.Thread`) | §5c |
| 3 Native→wasm ratio | 3 | 3 | 3 | 3 | Same LLVM; inferred tie |
| 4 Size and startup | 3 (JS glue, unmeasured) | 4 | 5 | 5 (unverified) | §5d |
| 5 Build cost here | 2 (emsdk absent; native test OK) | 3 (rustup; host tests after) | 3 | 1 | §3, §5e |
| 6 JS boundary | 4 | 4 | 4 | 3 | §5f |
| 7 Debug and host tests | 4 | 5 | 5 | 3 | §5g |
| 8 Maintenance | 3 | 4 | 3 (own glue) | 2 | §5g |
| **Total (unweighted)** | **28** | **31** | **31** | **22** | |

B and B′ tie. B′ trades size and boundary control against more hand-written glue, so the choice is deferred to 06. A trails by 3. The ranking changes only if build cost and threads are weighted down, or if 06 measures a real codegen gap (§8).

## 7. Recommendation and main reason

**Rust stable, `wasm32-unknown-unknown`, `core::arch::wasm32` SIMD128, a Worker pool of independent wasm instances for batches, `cargo test` on the host.** Glue (wasm-bindgen or raw) decided in 06.

Main reason: the performance evidence does not separate the options. Both compile through LLVM with SIMD128 and show the same ~1.15–1.5× native gap with SIMD (BLAKE3, §5a). The decision is therefore cost and threads. Rust is one `rustup` command from a wasm build and tests the same code natively. C++'s thread advantage only matters if one search must parallelise internally, which the batch design does not require.

**Caveats:**
- The 1.15–1.5× figure comes from wasmtime (BLAKE3) and 2019 browsers (Jangda), not from today's Chrome running a Rust bitboard kernel. Ticket 06 must measure.
- Rust bounds checks in hot loops can cost. Use fixed arrays and `get_unchecked` only where proven. Not measured.

## 8. What would change the recommendation

1. **06 shows Emscripten C++ at least ~15% faster** on the same kernel, same seeds, both run in Node (needs emsdk). The 15% threshold is my judgement.
2. **Intra-game parallelism is required** (e.g., parallel beam expansion within one game for latency) and nightly Rust is refused. Then C++/Emscripten stable pthreads wins.
3. **`rustup` install is blocked** on the dev machine and emsdk is available. Then C++ is cheaper to start.
4. **Ticket 04 picks an algorithm whose reference implementation is C++-only** (e.g., a library to reuse).
5. **Host cannot set COOP/COEP** (ticket 09). This hits shared-memory threads in either language, so it does not by itself favour C++. It matters only if intra-game threads are kept.

## 9. Open questions (unverified; not used to decide)

- emsdk download size and install time on this MinGW box.
- Rust wasm32 build time for this crate, and final `.wasm` size (no build run).
- Zig `@Vector` → wasm simd128 lowering and output quality; Zig release churn.
- Chrome/Firefox DWARF or source-map debugging quality for Rust vs C++ wasm.
- Whether rustc autovectorizes this kernel without explicit intrinsics.
- Memory64 for this workload: not needed for 20×10 (ticket 03 covers).
- Native-vs-wasm ratio for bitboard search on current V8/SpiderMonkey with SIMD. Measure in 06.
- Evals/s for a bitboard port. No number is claimed; 10⁶+/s is a guess to be measured, not a fact.
- AssemblyScript and hand-written WAT: not researched.

## 10. Sources

- Jangda, Powers, Berger, Guha, "Not So Fast: Analyzing the Performance of WebAssembly vs. Native Code", ATC'19 — https://arxiv.org/abs/1901.09056 (PDF https://arxiv.org/pdf/1901.09056). Table 1, §4, footnote 4.
- Emscripten SIMD — https://emscripten.org/docs/porting/simd.html
- Emscripten pthreads — https://emscripten.org/docs/porting/pthreads.html
- Rust `core::arch::wasm32` — https://doc.rust-lang.org/core/arch/wasm32/index.html
- Rust `wasm32-unknown-unknown` platform page — https://doc.rust-lang.org/rustc/platform-support/wasm32-unknown-unknown.html
- wasm-bindgen-rayon README — https://github.com/RReverser/wasm-bindgen-rayon
- MDN `Window.crossOriginIsolated` (SharedArrayBuffer gating) — https://developer.mozilla.org/en-US/docs/Web/API/Window/crossOriginIsolated
- Zig std `Thread.zig` (master) — https://raw.githubusercontent.com/ziglang/zig/master/lib/std/Thread.zig
- Zig documentation (master, read via summary) — https://ziglang.org/documentation/master/
- BLAKE3 PR #341 (Rust wasm32 SIMD, wasmtime benches) — https://github.com/BLAKE3-team/BLAKE3/pull/341
- Rust and WebAssembly book, "Shrinking .wasm Size" (unpublished copy) — https://rustwasm.github.io/book/reference/code-size.html
- Local: `TetrisSolver.py`, `tetris_engine.py`, `tetromino_data.py`, `TetrisGameGenerator.py`, `app.py`; Python benchmark run this session (script in /tmp, not committed).

Not retrieved: WebAssembly roadmap feature table (rendered empty); Chrome SIMD blog (404); MDN SharedArrayBuffer page (404); web search (rate-limited).
