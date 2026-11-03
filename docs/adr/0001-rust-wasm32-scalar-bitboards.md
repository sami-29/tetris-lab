# Rust on wasm32, scalar u32 column bitboards, raw exports

The browser rewrite needs ~10⁷ board evaluations per second inside a visitor's tab, so the language and representation were chosen against measurement rather than expectation: a throwaway spike (findings in `docs/research/06-bitboard-benchmark.md`) ported the shipped solver faithfully and benchmarked it on identical games. We ship **Rust stable, `wasm32-unknown-unknown`, raw `WebAssembly` exports, ten u32 column words, scalar code, one instance per Worker** — 16.4M evals/s in Node (0.030 ms per default game), 63 KB of wasm, against 8.6k evals/s for the numpy solver.

## Considered Options

- **C++ / Emscripten** — the operator's leaning. Rejected on the measured falsifier this decision set for itself: same kernel, same games, exact parity, C++ was 21.5M evals/s with `-march=native` against Rust's 27.7M, and 13.4M against 17.7M on default flags. No 15% win appeared, and emsdk is the heavier install. Revisit only if intra-game threading becomes a requirement or a tuned C++ kernel beats Rust by a measured margin.
- **wasm-bindgen glue** — rejected: the boundary is five exports carrying two typed arrays and packed integers, so the generated glue buys nothing at the cost of a build dependency and a second ABI vocabulary. The loader owns memory views instead, in one file.
- **u64 column words** — rejected: 2.1× slower in wasm (7.8M vs 16.4M evals/s), because i64 operations are legalised and their `clz`/`popcnt` are not single instructions. Twenty rows never needed more than twenty bits.
- **SIMD128** — rejected on measurement: a fully vectorised, parity-verified evaluator runs at 0.85× the scalar speed, because SIMD128 has no `clz` and heights dominate the score. One scalar build, no capability check, no second artifact.
- **Shared-memory threads** — rejected: a 1000-game batch completes in 30 ms on one core, so batches need no threads, which removes the nightly toolchain, `build-std` and the COOP/COEP requirement from the critical path.

## Consequences

- The build needs Rust stable plus the `wasm32-unknown-unknown` target; no nightly, no `build-std`, no emsdk.
- Because the ABI is hand-written, the TypeScript loader is the only place that touches `WebAssembly` and it must handle memory growth (views detach) and the `abi_version` handshake itself.
- The engine is clz/popcount-bound, so representation changes should be re-measured rather than assumed — in particular if the evaluator is replaced by a wider feature set, SIMD128 may deserve a second look.
