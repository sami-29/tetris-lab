# Ticket 06 spike — bitboard solver benchmark

Throwaway code from `.scratch/browser-tetris-lab/issues/06-prototype-bitboard-benchmark.md`.
Nothing here is wired into the site and nothing here is production quality: it exists to
produce the numbers in `.scratch/browser-tetris-lab/research/06-bitboard-benchmark.md`.

Every kernel is a faithful port of `TetrisSolver.py` + the live terms of
`tetris_engine.evaluate` — same beam schedule (1, 3, 7, 25), same iteration order, same
stable tie-breaking, same dead `_W_LINES` term, same eval counter. That is the whole point:
the speed ratio is measured on identical work, and every kernel must reproduce the Python
run's `(success, evals)` per seed exactly before its timing is believed.

## Layout

```text
prototype/
├── build.sh              build every variant (native + wasm), artifacts/ (gitignored)
├── run-bench.sh          reproduce the whole results table
├── summarize.py          aggregate raw runs -> data/results.csv, data/results.md
├── py/baseline.py        export the games file, measure the Python solver (as-is, lines_live)
├── rs/                   Rust kernel: lib + native bench bin + raw wasm exports
│   └── src/
│       ├── kernel.rs     bitboard beam search, generic over u32/u64 column words
│       ├── pieces.rs     shape literals copied from tetromino_data.py
│       ├── games.rs      parser for the exported games file
│       ├── simd.rs       SIMD128 evaluator (measured: slower than scalar)
│       ├── wasm_api.rs   wasm ABI: load_games / run_game / solve_all / parity_check
│       └── main.rs       native bench binary (JSONL rows + summary + --check gate)
├── cpp/t06_bench.cpp     the same kernel in C++, for the language comparison
├── wasm/bench.mjs        Node harness: per-game timing, batch path, instance restart, parity
└── data/                 games file, per-seed rows, summaries, results table (gitignored)
```

## Run it

```bash
bash prototype/run-bench.sh      # everything, ~5 minutes including the Python baselines
SKIP_PYTHON=1 bash prototype/run-bench.sh   # kernels only, reuse existing Python rows
```

Individual rows, and what each variant is:

| Artifact | How it is built | What it answers |
|---|---|---|
| `artifacts/bench-native-u32.exe` | `cargo build --release` (GNU host toolchain) | Rust native, default flags |
| `artifacts/bench-native-u32-native.exe` | `RUSTFLAGS=-Ctarget-cpu=native` | Rust native with lzcnt/popcnt |
| `artifacts/bench-native-u64-native.exe` | `RUSTFLAGS=-Ctarget-cpu=native --features word64` | u64 column words |
| `cpp/t06_bench_opt3.exe` | `g++ -O3 -std=c++20` | C++ native, default flags |
| `cpp/t06_bench_native.exe` | `g++ -O3 -march=native` | C++ native, native ISA |
| `artifacts/wasm-u32.wasm` | `cargo build --target wasm32-unknown-unknown` | the recommended engine |
| `artifacts/wasm-u64.wasm` | same, `--features word64` | u64 in wasm |
| `artifacts/wasm-u32-simd.wasm` | same + `RUSTFLAGS=-Ctarget-feature=+simd128 --features word32,simd` | does SIMD128 pay? (no) |
| `artifacts/wasm-u32-lines_live.wasm` | same, `--features word32,lines_live` | `_W_LINES` actually contributing |

## Gotchas found while building this

- **No MSVC linker on this box.** `rustup default stable` installs the MSVC host toolchain,
  which cannot link here (`link.exe` on PATH is MSYS coreutils), so native builds use
  `+stable-x86_64-pc-windows-gnu`. wasm builds do not need a linker at all.
- **`CARGO_HOME` is set to `E:\cargo`** in this environment while rustup lives in
  `C:\Users\sami-\.cargo`; the scripts pass `CARGO_HOME` explicitly to avoid rustup's
  "not installed at" error.
- **Native default flags are unfair to native.** Without `-Ctarget-cpu=native`, `leading_zeros`
  compiles to `bsr`+branch and `count_ones` to no `popcnt`, while wasm's `i32.clz`/`i32.popcnt`
  map to the best available instruction. Both native rows are reported for that reason.
- **SIMD128 has no `clz`,** which is why the vectorised evaluator loses: heights are the
  dominant term and they cost ~20 vector ops per four columns. The SIMD build also carries a
  `parity_check` export that scores 41,002 boards both ways; keep that gate if you touch it.
- **Node `Uint32Array` views detach when wasm memory grows.** Create views after the last
  `alloc`, never before.
- **`solve_all` needs an explicit game count** — it writes into a caller-sized buffer, and an
  overrun silently corrupts whatever follows.

## Results

The table, the interpretation and the decision implications live in
`.scratch/browser-tetris-lab/research/06-bitboard-benchmark.md`; the raw table is regenerated
as `prototype/data/results.md`.
