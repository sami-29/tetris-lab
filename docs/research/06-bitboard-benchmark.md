# 06 — Prototype: bitboard solver speed proof

Status: spike complete. The harness lived in `prototype/` and its raw runs in `prototype/data/`, both deleted at cutover; the results table (`results.md` under it) was never tracked. Recover any of it with `git show 4bb9354:prototype/<path>`. Labels: **measured** = produced by this harness on this machine in this session; **inferred** = arithmetic or reasoning from those numbers; **unverified** = not measured.

## 1. The question

What does the rewrite actually buy, in numbers: evaluations/sec, one game at the app's default settings, a 1000-game batch — Python as-is versus a bitboard kernel in the recommended language, natively and in wasm, with the SIMD128 and u32/u64 questions settled by measurement. Also: the cost of replacing a terminated worker's wasm instance (left open by 05).

## 2. Environment (measured)

| Item | Value |
|---|---|
| CPU / OS | AMD Ryzen 7 5700X (8C/16T), Windows, 16 logical cores |
| Python | 3.11.9, numpy 2.4.6 |
| Node | v24.21.0 (V8 13.6) |
| C++ | g++ 13.2.0 (MinGW-w64, posix-seh) — already on PATH |
| Rust | rustc 1.99.1 stable, host `x86_64-pc-windows-gnu`, target `wasm32-unknown-unknown` |
| Rust install cost | ~2 min: `rustup default stable` + `rustup target add wasm32-unknown-unknown`, plus the GNU host toolchain because the box has no MSVC linker (`link.exe` on PATH is MSYS coreutils, not MSVC) |
| C++ install cost | zero (compiler present) |
| Not installed | emcc/emsdk, clang, cmake, zig — so **no C++ wasm row** exists in this table |

Both languages ran natively, but only Rust produced a wasm artifact. g++ installed faster (it was already there); Rust was the only wasm toolchain that installed in minutes, the same conclusion ticket 01 reached on build cost.

## 3. Method (what makes the comparison honest)

- **Identical games.** The spike's Python baseline exporter wrote 1000 games (app defaults: goal 8, 40 pieces, `initial_height_max=7`, 7-bag with the S/Z reshuffle) using the repo's own `TetrisGameGenerator`, to `games_goal8_p40_seed0-999.txt` (deleted with the harness). Every kernel parsed that file, so no RNG is reimplemented and no kernel plays a different game.
- **Faithful port.** Beam schedule (1, 3, 7, 25), iteration order state → rotation → column, stable descending sort, the dead `_W_LINES` term (zero unless built with `lines_live`), and the eval counter incremented at the same point as `TetrisSolver` (spawn-legal placements only).
- **Correctness gate, not a smoke test.** Every kernel must reproduce the Python run's `(success, evals)` per seed exactly. That gate caught two real bugs during the spike (a mistranscribed J rotation, and a vectorised `clz` that shifted the wrong way), which is why the numbers below are trustworthy. Final state: 0 mismatches on 1000 seeds for every scalar kernel, 0 for the SIMD build against the scalar path over 41,002 boards.
- **Noise handling.** Single runs on this box drifted by up to ~20% between minutes, enough to flip close comparisons. `run-bench.sh` therefore runs **interleaved sweeps** — every variant back to back, nine times per sweep — for 3 sessions × 5 sweeps = **15 pooled runs per variant**, medians reported. Absolute numbers are still only as good as the machine; the ratios between rows from the same sweeps are the load-bearing part.
- **Python measured twice**: shipped code, and with `_W_LINES` live (a copy of `_beam_search` that scores `+1000 × lines_this_move`; asserted to reproduce the shipped solver when the weight is 0).
- All kernels spend exactly 612,847 evaluations per 1000 games, so a row's evals/s is a pure cost ratio.

## 4. Results (measured)

Goal 8, 40 pieces, 1000 seeds, median of 15 interleaved runs (Python: one 1000-game run). `vs python` = evals/s ÷ 8,635.

| solver | engine | evals/s | vs python | ms/game (median) | p90 ms | 1000-game batch | wasm bytes |
|---|---|---|---|---|---|---|---|
| python-asis | numpy | 8,635 | 1× | 66.59 | 85.5 | 71.0 s | — |
| python-lines_live | numpy | 8,373 | 0.97× | 69.50 | 88.1 | 73.0 s | — |
| rs-native-u32 | native, default flags | 17,738,539 | 2,054× | 0.0305 | 0.048 | — | — |
| rs-native-u32-native | native, `target-cpu=native` | **27,662,916** | **3,204×** | **0.0198** | 0.029 | — | — |
| rs-native-u64-native | native, `target-cpu=native` | 24,769,401 | 2,869× | 0.0230 | 0.033 | — | — |
| cpp-native-opt3 | C++, `-O3` | 13,407,549 | 1,553× | 0.0400 | 0.066 | — | — |
| cpp-native-march | C++, `-O3 -march=native` | 21,480,491 | 2,488× | 0.0263 | 0.037 | — | — |
| wasm-u32 | wasm, V8 | 16,387,598 | 1,898× | 0.0296 | 0.049 | 30.3 ms (32,978 games/s) | 62,958 |
| wasm-u64 | wasm, V8 | 7,804,264 | 904× | 0.0659 | 0.112 | 68.1 ms | 60,915 |
| wasm-u32-simd | wasm + SIMD128 | 13,912,310 | 1,611× | 0.0399 | 0.054 | 36.0 ms | 65,420 |
| wasm-u32-lines_live | wasm, `_W_LINES` live | 16,006,717 | 1,854× | 0.0302 | 0.049 | 29.2 ms | 62,965 |

Successful solves: 1000/1000 in every row. Batch figures are one `solve_all` call for all 1000 games.

Worker-restart cost, wasm u32 (was 05's open question): compile a module from bytes 0.39 ms, instantiate a fresh instance **0.06 ms**, and a full worker-like restart (fresh instance + parse the 270 KB games file + first solve) **1.6 ms**.

## 5. What the numbers decide

**a. Throughput: the rewrite buys ~1,900× in wasm (~3,200× native), and the browser is not the slow part.** wasm u32 runs a default game in 0.030 ms against Python's 66.6 ms, and a 1000-game batch in 30 ms against Python's 71 s on one core. A 1000-game batch becomes one frame's work rather than a progress bar. Inferred consequence for 04: at 1.6–2.0 × 10⁷ evals/s the beam width stops being a speed constraint. A game costs ~600 evals today; widening the beam to 400 or 1,600 costs ~10–40k evals, i.e. ~1–3 ms — the width-scaling experiment 04 asks for is affordable in the browser, and quality (heuristic, dedup, width) is the only lever left.

**b. Native versus wasm: 1.69× against the best native build, and wasm is level with a default-flagged one.** Best native (27.7M, `target-cpu=native`) beats wasm (16.4M) by 1.69×; native with default flags (17.7M) is only 8% ahead of wasm. The mechanism is codegen, not wasm: rustc targeting baseline x86-64 emits `bsr`+branch for `leading_zeros` and no `popcnt`, while wasm's `i32.clz`/`i32.popcnt` let V8 choose the best available instruction at runtime. So "wasm costs ~1.7× against an optimally-flagged native build, ~1.1× against a default-flagged one", both ~1,900–3,200× Python. Browser numbers will sit below Node's; nothing here suggests that gap matters.

**c. u32 columns, not u64.** In wasm u32 is 2.1× faster (16.4M vs 7.8M evals/s; 0.030 vs 0.066 ms/game) — i64 ops are legalised and their `clz`/`popcnt` are not single instructions. Native agrees in direction, mildly (27.7M vs 24.8M, 1.12×). Board arithmetic never needed more than 20 bits. **Decide: u32.**

**d. SIMD128 does not pay — drop it.** 13.9M vs 16.4M evals/s (0.85×, i.e. −15%), 0.040 vs 0.030 ms/game, 36 vs 30 ms per batch, +2.5 KB of binary. The cause is structural: SIMD128 has no `clz`, and this evaluator is clz/popcount-bound, so the vector `clz` costs ~20 ops per four columns against four scalar `lzcnt`. The correctly-vectorised build (heights, holes and bumpiness all verified against the scalar path on 41,002 boards) still loses. 03 called SIMD "optional, ~1.0–1.5×"; it measures **0.85×**. **Decide: scalar only — no second SIMD build, no capability check.** If the evaluator is later replaced by a wider Dellacherie-Thiery feature set (04), re-measure before adding SIMD.

**e. Rust at least matches C++ on this kernel, so 01's 15% threshold is not met.** Same kernel, same games, exact parity: Rust `target-cpu=native` 27.7M vs C++ `-march=native` 21.5M (Rust 1.29×); default flags 17.7M vs 13.4M (Rust 1.32×). Caveat: the C++ port is a line-by-line transcription written second and not tuned (single TU, no PGO), so read the direction, not the magnitude. The 15% win that would flip 01's recommendation does not appear; the operator's C++ leaning has no performance argument behind it. **Ticket 07 input.**

**f. `_W_LINES` live versus dead is worth ~4% of Python wall time and ~0.3% of work.** Python 8,373 vs 8,635 evals/s (69.5 vs 66.6 ms); 611,002 vs 612,847 evals; 1000/1000 solved either way at goal 8. The dead term is a correctness bug in the evaluator, not a hidden speed win: the speed claim above is safe against it, and the wasm `lines_live` build reproduces the `lines_live` Python run exactly (0/1000 mismatches), so both baselines are reproducible. Its real cost is quality, which is 04's territory.

**g. Worker restart is free.** `terminate()` plus a fresh instance is 0.06 ms; the 1.6 ms "cold" number is almost entirely re-parsing the games list, which the 05 design can avoid by sending seeds rather than a parsed file. Cancellation by `terminate()` costs nothing measurable.

## 6. Limits of this evidence

- Node 24 / V8 only — no Chrome, Firefox or Safari, and no Worker or WASM-thread context. Expect the browser to land below the wasm row.
- One core, one game at a time. No threads, no SharedArrayBuffer; the batch path is a single call in a single instance.
- Quality is unchanged by construction: this is the shipped beam search and the shipped heuristic, so success rate is identical (1000/1000 at goal 8) by design. Nothing here says whether the new solver wins on lines — that is 04's ablation.
- The C++ comparison is native-only (no emcc), and the C++ kernel is a transcription, not a tuned implementation.
- Absolute throughput varies with the machine's mood (up to ~20% between sessions); every figure is a pooled median and the ratios are the trustworthy part.

## 7. Reproduce

The harness is deleted at cutover, so nothing here runs on a fresh clone. Recover
each file with `git show 4bb9354:prototype/<path>` — the `run-bench.sh` orchestrator,
the Python baselines, the native and wasm kernels, the C++ kernel, and the exported
games file — then run the harness's own commands. The table above is the preserved
evidence; the commands were never re-run after the cutover.

Per-game rows (one JSON object per seed) landed in the spike's `data/*.jsonl`; the aggregated table was `results.md` beside them. Both were deleted with the harness; recover them with `git show 4bb9354:prototype/<path>` when they were tracked, and see the status line for the rest.

## 8. Answer to the ticket

The rewrite buys **~1,900× in wasm and ~3,200× natively** on the default workload (66.6 ms → 0.030 ms per game; 71 s → 30 ms per 1000 games), using **u32 column words, scalar wasm, Rust**. SIMD128 loses 15%; u64 columns lose 2.1× in wasm; C++ has no speed advantage over Rust; wasm sits 1.69× below an optimally-flagged native build and ~8% behind a default-flagged one; replacing a terminated worker's instance costs 0.06 ms. Throughput is solved, so ticket 04's width/heuristic/dedup choices — not the engine — decide final quality.
