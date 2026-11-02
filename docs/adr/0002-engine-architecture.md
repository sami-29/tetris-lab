# Engine architecture: layered beam over bitboards, swappable evaluator

The solver searches a known piece sequence, so each beam layer is one piece index and no sampling is needed. We ship a **layered beam search over u32 column bitboards with per-layer deduplication, a no-suicide rule, an exact 3-piece tail search, and an evaluator behind a trait** — two implementations (`Heuristic` as the ablation control, `DellacherieThiery` with eight fixed-point features), with the shipped default decided by the ablation rather than by preference.

The code lives in one Rust workspace plus the web app: `engine/` (board, pieces, beam, dedup, evaluators, tail — pure, no I/O, host-testable), `engine-wasm/` (the exported ABI and nothing else), `bench/` (native CI and ablation runner reusing `engine/` exactly as the wasm build does), `web/` (Vite, vanilla TypeScript, Canvas 2D). `engine/` never imports wasm or JS types; `engine-wasm/` never contains search or scoring logic. That split is what makes "the browser number is not a guess" structural.

## Considered Options

- **Port the shipped heuristic and stop** — rejected: the measured ceiling makes beam width nearly free (~1–3 ms for a 1,600-wide beam), while the current evaluator is the known quality limit (a dead lines term, holes weighted at 0.7 cells of height), so shipping it would leave the cheap win unused.
- **Dedup + wide beam, skip the evaluator rewrite and tail** — rejected for the same reason from the other side: it spends the speed budget without touching the term that decides quality.
- **UCT/MCTS** — rejected in `.scratch/browser-tetris-lab/research/04-algorithm-frontier.md`: tuned for versus play, needs seeded RNG, unclear at fixed small budgets.
- **Shipping move lists for every game** — rejected: 1000 games × a few hundred bytes is needless traffic for data no visitor replays. Summaries cross the boundary; move lists are fetched for the one game being watched.

## Consequences

- **Determinism is observable.** The engine contains no RNG, and ties break by insertion order, so `(piece, rotation, column)` visit order is part of the contract: changing it changes results and must show up as a deliberate golden-fingerprint update.
- **Scoring is fixed point.** Features are integer counts and weights are i32 at a documented scale, accumulated in i64, so the native bench and the browser agree exactly and the ablation compares like with like.
- **Dedup keeps the dominating state.** The key is the board alone; on a collision the state with more lines cleared wins, since the board is the whole future and the score is a function of the board.
- **The ABI has a failure contract.** Expected failures return typed codes in the packed result; bugs abort, and a trap is defined as "respawn the worker and report it" (a cold worker costs ~1.6 ms). Every module exports `abi_version()` and the loader refuses to run on a mismatch.
- **Game-over convention is a parameter,** not a constant: spawn-blocked versus top-out changes published scores, and that product call belongs to the product-scope decision.
- **CI guards the fingerprint, not the clock.** Per-seed success and eval counts over the fixed 1000-seed set are a hard check on the native `bench/` binary; wall time is reported with a loose sanity bound, because wall-clock assertions on shared runners are flaky.
