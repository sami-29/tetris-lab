# 04 — Algorithm frontier for the fixed-sequence line-goal problem

Status: research complete (read-only; no code changed, no installs). Labels: **verified** = a primary source (paper, repo file, or code in this repo) states it; **inferred** = my reasoning from those sources; **unverified** = not checked, or only a secondary/blog source.

## 1. The question

Fixed 20×10 board, fixed piece sequence drawn from a 7-bag, goal = reach N cleared lines within the sequence (or maximise lines). Which algorithm family should the browser rewrite use, what are the top optimisations, and what limits quality today: heuristic, beam diversity, or raw throughput?

Problem facts:
- Offline Tetris (sequence known) is NP-complete to maximise cleared rows, and inapproximable within p^(1-ε) for p pieces. **verified** — Demaine, Hohenberger, Liben-Nowell, "Tetris is Hard, Even to Approximate", https://arxiv.org/abs/cs/0210020. So no polynomial algorithm and no useful guarantee: the choice is empirical.
- Board size bound: 4 cells per piece and 10 cells per row means P pieces can clear at most 4P/10 rows. **inferred** (arithmetic). For P=100 the ceiling is 40 lines, so a goal of 40 is only reachable with zero wasted cells; realistic goals sit well below that (see §6 for calibration).

## 2. Options compared

- **O1 — Beam search, bitboard state, dedup + better features.** Keep the layered beam over the known sequence; represent boards as bit columns; deduplicate by board hash per layer; use a Dellacherie-family evaluator.
- **O2 — UCT / MCTS over the sequence.** Tree over (board, piece index); rollouts or leaf evaluation by heuristic. Cold Clear and Cai et al. use this family.
- **O3 — A\*/IDA\*.** Needs an admissible bound on remaining lines. No cheap one exists in this literature (inferred from the NP-hardness and inapproximability result above; I found no Tetris A\* paper with a usable bound — **unverified** negative).
- **O4 — Exact layered search with state merging (ZDD/DP-style) with bounded lookahead.** Works for perfect-clear enumeration on 4-row fields; does not scale to a 20-row goal over 100+ pieces (see §4).
- **O5 — Policy/value learning (CBMPI, cross-entropy on weights).** Not a solver for a fixed sequence on its own; it is the tuning method for the O1 evaluator (see §3).

## 3. Evidence on evaluators, search and results

**Heuristic quality (what the best numbers come from).**
- Dellacherie's one-piece controller, reverse-engineered by Thiery & Scherrer: score = −(landing height) + (eroded piece cells) − (row transitions) − (column transitions) − 4×(holes) − (cumulative wells). **verified** — https://inria.hal.science/inria-00418954/document (Section 2.1).
- Dellacherie's score is 660,000 ± 27% lines on Fahey's original simulator, but **5,200,000 ± 20%** on the simplified simulator; the gap comes from implementation rules (whether moves that lead to game over are evaluated; whether the 10×19 vs 10×20 top-out convention is used). **verified** — same paper, Section 3.3. Implication: any cross-solver comparison must fix these rules, and a "lines" number from the literature is not comparable to ours without the same rules.
- Thiery & Scherrer tuned a 9-feature set (Dellacherie + extras) by cross-entropy to ~35,000,000 ± 20% lines (one-piece controller, endless game). **verified** — same paper and Gabillon et al. 2013, https://mohammadghavamzadeh.github.io/PUBLICATIONS/nips13-tetris.pdf (which also states this figure). Gabillon et al.'s CBMPI/DPI reaches ~4,300 lines on a 10×10 board with the same D-T features; on 10×20 it is comparable to CE with ~1/6 the samples. **verified**.
- Two-piece lookahead (Fahey) reported 7,200,000 lines, single game. **verified** (same Thiery paper, one game only, so weak).
- Variance: score is roughly geometric with sd ≈ mean; 56 games give ±27% at 95%. **verified** (Thiery & Scherrer, Section 3.2). Consequence: the comparison protocol needs many seeds and paired tests.

**Search families.**
- UCT for Tetris with a fixed piece sequence (Cai, Zhang, Nebel, AISB 2011): 100-piece games, transposition database of visited states, hole-count pruning of actions. **verified** — https://gki.informatik.uni-freiburg.de/papers/cai-zhang-nebel-aisb11.pdf. Their score curves are only in figures; I did not extract absolute numbers (**unverified**).
- MCTS with heuristic rollouts (Naing, Truong, Zeng, Stanford CS221): reports clearing over 3,500 lines; setting and piece model are not clear from the abstract (**unverified** as a comparison point). https://thawsitt.me/files/Tetris_AI_CS221_final_paper.pdf
- Cold Clear (MinusKelvin, Rust, MPL-2.0, archived): a DAG-based MCTS with a 30+ feature, GA-tuned evaluator; it also merges transpositions and has a separate perfect-clear search (PcLooper). **verified** from the repo README and the komorinfo write-up of the code (Japanese source, 2021-09-30 snapshot, file `bot/src/dag.rs`): https://github.com/MinusKelvin/cold-clear and https://komorinfo.com/blog/cold-clear-search-algorithm/. Cold Clear is a versus bot (online, next-queue visible), so its node selection is tuned for speed-of-attack, not for a line goal.
- Cold Clear 2 is a rewrite: https://github.com/MinusKelvin/cold-clear-2 (**verified** existence; I did not read its search).
- Diverse beam search (Vijayakumar et al., 2016) adds an explicit diversity term to beam ranking; it is from neural sequence decoding, not Tetris. **verified** for the paper's existence and claim: https://arxiv.org/abs/1610.02424. Its transfer to Tetris is **inferred**.
- Exact layered merge: knewjade's 4×10 perfect-clear enumeration merges states with identical boards, producing 522,230,555 solutions in ~1.1 ms on a Mac mini 2023 with 4,168 states and 7,837 transitions. **verified** — https://gist.github.com/knewjade/f10b6f0d627eb482e0150fcf1b5de99b. This works because the field is only 4 rows and the target is a fixed final board. It is not evidence for a 20-row line goal.
- AlphaEvolve blog (bufferof.com): a beam of width k=200 over bitboards with transposition hashing, evolved heuristic; reports ~0.95 s for N=8 pieces. **unverified** (blog, no code, no comparison protocol; numbers not reproducible from the text).

**Bitboard representation (what it enables).**
- Cold Clear's bitboard benchmark repo (board-repr-perft) compares naive, row-bits (u16 ×40), and column-bits (u64 ×10) boards on Dellacherie and "advance" workloads; the column representation is the one that matters, and its pext variant is slower on pre-Zen-3 AMD. **verified** — https://github.com/MinusKelvin/board-repr-perft (README; absolute timings are in SVG plots I did not extract: **unverified**).
- Column-major code shown in the repo's `src/column_bits.rs` **verified** from the source:
  - line clear mask = AND of all column words: `columns.iter().fold(!0, |a, b| a & b)`;
  - column height = `64 - c.leading_zeros()`;
  - holes in a column = `(!c & ((1 << (64 - c.leading_zeros())) - 1)).count_ones()`;
  - column transitions / row transitions = popcount of XOR between adjacent words;
  - collapsing lines = shift per column using the trailing-zero offset.
- For a 20-row board, each column fits in a u32; ten u32 columns = 40 bytes; no pext needed. **inferred** (row count and word size are arithmetic; WASM handles i32 natively — the choice between u32 and u64 should be measured).

**What the current Python solver does (verified by reading the code):**
- `TetrisSolver.py`: beam widths (1, 3, 7, 25), restart at next width if beam fails; candidates scored by `engine.evaluate(tmp)` with `tmp` after line clear; no dedup of boards; every candidate copies a 20×10 uint8 array and runs numpy routines per placement.
- `tetris_engine.py`: `_W_LINES=1000` multiplies `lines = count of full rows`. But `evaluate` is always called on the board returned by `clear_lines`, which has already removed full rows, so **this lines term is always zero**. **verified** by code reading. The live terms are −51×(sum of column heights), −35×holes, −18×bumpiness.
- `drop_row` scans rows with `np.any` in a Python loop; `get_valid_columns` is a list comprehension over columns. **verified**.

**Heuristic sensitivity (metric matters).** On the Tetris Bench public harness (30 seeds, gravity paused, score not lines), a one-ply greedy heuristic scored 274,207 ± 14,326 and a Dellacherie-style evaluator 269,744 ± 10,020, with two-ply search at 267,675 ± 2,019. **verified** — https://luca.lowndes.net/tetris-bench. The ranking is within noise, and the metric is not lines-to-goal. Lesson: heuristic changes alone move scores little on noisy metrics; measure on the target metric.

## 4. Scoring the options

Criteria (weights are my judgement, **inferred**): solution quality at fixed compute (40%); determinism and reproducibility for the protocol (15%); predictable cost in WASM with no threads (20%); fit with a known finite sequence (15%); implementation effort (10%).

| Option | Quality at fixed compute | Determinism | WASM cost | Uses known sequence | Effort | Notes |
|---|---|---|---|---|---|---|
| O1 beam + bitboard + dedup + D-T eval | High (inferred; the literature's best one-piece evaluators are beam/greedy-like) | Full (no RNG) | Predictable, bounded by width | Yes (layer = piece index) | Medium | Matches current code's shape; easy to compare |
| O2 UCT/MCTS | Good in long games, unclear at fixed small budget (Cai results only in figures) | Needs seeded RNG | Variable; many allocations | Partially | High | Cold Clear is tuned for versus play |
| O3 A\*/IDA\* | No admissible bound known | Yes | Cheap per node | Yes | High | Dead end on this objective (inferred) |
| O4 exact merged layers | Exact on small horizons only | Yes | Memory explodes with width | Yes | High | Use only as a bounded tail solver for the last 2–3 pieces (inferred) |
| O5 learned evaluator (CE/CBMPI) | Best published numbers (verified, endless game) | Depends on training | Weights are cheap at run time | Not needed | Medium–high offline | Use to tune O1's weights, not as the search |

## 5. Recommendation

**Adopt O1: a layered beam search over bitboard states with per-layer board deduplication and a Dellacherie-Thiery evaluator, with the width schedule kept but extended, plus a tail exact search over the last 2–3 known pieces.** The sequence is known, so each beam layer is a deterministic piece index and no sampling is needed.

Top three concrete optimisations, in order:
1. **Dedup by board hash inside each beam layer, keeping the best score per board (and per lines-cleared count).** Cold Clear and Cai et al. both rely on state merging; the beam without it can fill with transpositions of one board. Expected effect: more distinct boards per width (**inferred**; measure with the width-scaling test in §6).
2. **Bitboard columns (u32 ×10 for 20 rows) with incremental features.** Line clear by AND of columns; height by clz; holes and transitions by popcount (formulas in §3). Precompute per-piece placement masks for all rotation/column pairs. Expected: large throughput gain over numpy per-candidate work (**inferred**, not measured; the Python per-candidate cost is multiple numpy calls, so the gap is likely orders of magnitude).
3. **Fix the evaluator: drop the dead lines term; use the Dellacherie-Thiery feature set (landing height, eroded piece cells, row/column transitions, holes, cumulative wells, plus the hole-depth and rows-with-holes features from Thiery) with weights tuned by cross-entropy on a separate tuning seed set.** Weights start from Dellacherie's published values (verified). Tune on seeds disjoint from the test seeds.

Also: keep the game-over rule explicit (spawn-blocked versus top-out) and never choose a move that ends the game when an alternative exists (Thiery & Scherrer show this changes Dellacherie's score by ~6×; **verified**).

## 6. Comparison protocol against the current Python solver

Purpose: decide whether the new solver beats the current one, and which of the three changes carries the gain.

Fixed setup (all runs):
- Board: empty 20×10, rows 0..19, top-out = spawn cell blocked (state this in the code and in the output).
- Piece source: 7-bag shuffle from `random.Random(seed)`; sequence length P=100 pieces (Cai et al. use 100-piece games), plus P=200 as a second horizon. Record the exact generator.
- Seeds: tuning set = seeds 10000–10999 (only for CE weight tuning and goal calibration); test set = seeds 0–999. Never tune on test seeds.
- Goal calibration: on the tuning set, run the current Python solver at schedule (1,3,7,25) and choose goals G so its success rate is ~30–70%. Report G for P=100 and P=200. With P=100 the ceiling is 40 lines (§1), so expect goals around 15–30 (**inferred**; confirm in pilot).
- Repetitions: solvers are deterministic given the sequence, so one run per seed. If any tie-break uses RNG, run 5 tie-break seeds and report the pooled mean.
- Hardware and versions: record CPU model, OS, Python 3.11 and numpy version for the baseline; browser, WASM build flags and wasm binary size for the new solver.

Metrics (in order of priority):
1. Success rate at goal G (fraction of the 1,000 test sequences where the goal is reached within the sequence). Report with Wilson 95% CI.
2. Lines reached on failure: mean and median lines before the beam dies or the sequence ends.
3. Cost: evaluations per solve, wall time per solve (median, p90), and evaluations per second.
4. Validity: replay each returned move list on an independent simulator; it must reproduce the claimed lines and never overlap or leave the board.

Comparisons (same 1,000 test seeds, same G):
- A. Baseline: current Python solver as-is (no algorithm change; the ticket excludes porting it).
- B. New solver, same width schedule (1,3,7,25), all three changes on.
- C. Ablation at the same schedule: (i) bitboard only; (ii) bitboard + dedup; (iii) bitboard + dedup + new evaluator. This attributes gain to each change.
- D. Width-scaling curve for the new solver: widths 25, 100, 400, 1,600 at fixed heuristic. If success keeps rising with width, throughput is the limit; if it flattens early, heuristic or diversity is the limit.
- E. Equal-budget test: give A and B the same evaluation count (not only the same time), to remove the speed advantage from the quality comparison.

Decision rule (pre-register before running):
- Ship O1 if B beats A on success rate by ≥10 percentage points at equal goal with a paired McNemar p<0.01, OR matches A's success rate with ≥5× lower median wall time.
- If the width-scaling curve (D) is flat, do not spend effort on throughput beyond the bitboard port; invest in the evaluator.
- If dedup (C-ii) changes nothing at equal evaluations, drop it and revisit beam diversity with a diversity term (Vijayakumar-style) instead.

Statistics: paired comparison per seed (same sequence). Report mean paired difference of lines with bootstrap 95% CI (10,000 resamples). Thiery & Scherrer's variance remark applies: with sd ≈ mean, expect wide CIs unless N is large; 1,000 seeds gives roughly ±6% relative on the mean (**inferred** from their k/√N formula with k=2).

Output file layout for the run: one CSV row per (solver, seed, goal, P, width) with success, lines, evals, ms; plus a summary JSON with CIs. Store under a local scratch directory outside the repository (not in the repo's data files).

## 7. What limits quality (answer)

- **Heuristic and diversity are the likely limits; throughput is a secondary, capping one.** Reasoning (**inferred**): the current evaluator wastes its lines term, gives holes a weight that is small relative to aggregate height (−35 per hole vs −51 per unit of height sum, so one hole is about 0.7 cells of height), and the beam has no dedup, so width 25 can be filled with transpositions. Throughput caps width at 25, which the Python code cannot raise cheaply.
- The published numbers point the same way: the best one-piece controllers gain most from features and weights (Dellacherie, then CE-tuned D-T), not from wider search alone (verified, Thiery & Scherrer and Gabillon et al.).
- This is a prior, not a result. §6 D and C decide it with measurements.

## Open questions

- **unverified**: absolute throughput numbers for bitboard vs numpy on this machine (no measurement taken; the board-repr-perft figures are SVG-only).
- **unverified**: whether a 20-row-goal beam at width 25 with the current evaluator is near its ceiling or far from it. Needs the Python baseline run in §6A.
- **unverified**: a canonical "NES-style 100-piece line-clear benchmark" with published line counts. I found the Cai et al. 100-piece setting (no extracted numbers), the StackRabbit NES AI (search + value iteration; no numbers extracted, https://github.com/GregoryCannon/StackRabbit), and Tetris Bench (different metric). I did not find a published fixed-sequence line-goal benchmark with seeds.
- **unverified**: the knewjade solution-finder and bitris repos (https://github.com/knewjade/solution-finder, https://github.com/knewjade/bitris) as implementation references for placement tables; I did not read their code.
- **unverified**: whether the Cold Clear 2 search improves on Cold Clear's MCTS for fixed-sequence play.
- **unverified**: whether u32 vs u64 columns is faster under WASM; measure in ticket 06.
- **open**: game-over convention to adopt (Thiery & Scherrer's 10×19 vs 10×20 finding) — a product decision for ticket 08, not a research one.
