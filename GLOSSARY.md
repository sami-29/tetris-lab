# Tetris Lab

Vocabulary here is the engine's own — board, search and scoring — plus the product terms the site exposes. Each term's `_Avoid_` list names the near-synonyms that must not stand in for it.

The first sections are the engine: the solver that answers one question — given a 20×10 board and a known piece sequence, can it clear a target number of lines, and how?

## Product

**Lab**:
The site as a whole: one page that solves a single game, runs the Dataset, and explains the search. A visitor arriving lands in it, not on a menu.
_Avoid_: app, tool, demo, portfolio page

**Run**:
One batch of solves over the whole Dataset, reported as a win rate and timings. Distinct from a Solve, which is one game.
_Avoid_: simulation, batch, job, sweep

**Dataset**:
The fixed, committed set of seeded games that every Run and every quoted figure is measured on. Its test seeds and its tuning seeds are disjoint parts of it.
_Avoid_: seeds, corpus, benchmark set, sample

**Dataset configuration**:
One committed games file, identified by its settings — goal, piece count, height cap and seed range — because a seed names a game only within one configuration. Distinct from a split, which is a choice of seeds inside a configuration.
_Avoid_: split, subset, batch

**Split**:
One of the two disjoint seed ranges inside a Dataset configuration: the test split, which a Run and the Reference run measure, and the tuning split, which only tuning and validation jobs touch.
_Avoid_: subset, partition, half, fold

**Reference run**:
The committed result of pushing the Dataset through the native harness, labelled with the machine, date and runtime it was measured on, and shown beside a visitor's own Run.
_Avoid_: baseline, benchmark, leaderboard, target

**Playground**:
The visitor-settable game: seed, goal, piece count, starting height and board presets. In v2 it also gains a board editor; in v1 it does not.
_Avoid_: editor, settings, config

**Explainer**:
The written account of how the search works, rewritten to match the engine actually shipped, living in sections of the same page as inline notes on the things on screen.
_Avoid_: docs, how-it-works page, wiki

## Validation

**Dataset fingerprint**:
The 64-bit value computed from every game in a Dataset configuration, whose board words and sequences it identifies; a change to any game moves it, reformatting the file does not.
_Avoid_: checksum, hash, digest

**Ablation**:
A comparison run that switches one engine component off on identical games at a fixed width, to attribute quality to that component.
_Avoid_: experiment, variant test, A/B

**Gate**:
A check that must pass before a build is accepted, whose failure blocks it — as opposed to an on-demand measurement that only reports.
_Avoid_: check, assertion, guard

**Width cap**:
The largest Beam width the expert control offers, fixed by the width curve's time ceiling rather than by taste.
_Avoid_: max width, limit, ceiling

## Language

**Board**:
The 20×10 playfield, held as ten column words. Row 0 is the top row; a filled cell is a set bit.
_Avoid_: grid, field, matrix

**Column word**:
One u32 per board column, bit *n* = row 19−*n* (bit 0 is the bottom row). The whole board is 40 bytes.
_Avoid_: column, bitmap, mask

**Seed**:
The integer that deterministically fixes one game: its starting board (with the height cap) and its piece sequence (on its own). Neighbouring seeds give unrelated games.
_Avoid_: id, index, number

**Pre-fill**:
The random play that builds a game's starting board: pieces dropped from the top, lines cleared as they complete, until a piece would come to rest above the height cap, that piece never landing.
_Avoid_: fill, initial board, setup

**Generator**:
The one function turning seed, piece count and height cap into a board and a sequence, shared by the exporter that writes the Dataset and the wasm the Playground runs.
_Avoid_: RNG, factory, builder

**Domain**:
The settings range the generator accepts — seed any u32, piece count 1–200, height cap 2–19 — outside which it reports an error rather than naming a different game. The Playground's controls stay inside it.
_Avoid_: limits, constraints, bounds

**Sequence**:
The known order of pieces for one game, drawn from a 7-bag and fixed before the search starts. A bag is shuffled again when it would begin with the S or Z that ended the previous bag.
_Avoid_: queue, bag

**Goal**:
The number of lines a solve must clear to win.
_Avoid_: target, threshold

**Lines cleared**:
Cumulative full rows removed since the game started, as opposed to lines removed by one placement.
_Avoid_: score, lines

## Search

**Placement**:
One legal rest of one piece, identified by piece, rotation and column. The unit the beam expands.
_Avoid_: move, candidate, action

**Move**:
A placement that survives into the returned solution, and therefore the unit a replay consumes.
_Avoid_: step, decision

**Beam width**:
How many states survive each layer. Narrower beams are cheaper and weaker.
_Avoid_: depth, budget

**Layer**:
The beam stage reached after placing piece *i* of the sequence. Deduplication happens inside one layer, never across layers.
_Avoid_: level, generation, ply

**Dedup**:
Removing duplicate boards inside a layer, keeping the state that has cleared more lines.
_Avoid_: prune, merge, transposition table

**Tail search**:
Exact search over the last pieces of the sequence, where a beam's approximation is no longer worth its speed.
_Avoid_: endgame, finish

**No-suicide rule**:
A placement that ends the game is only legal when no other placement is legal.
_Avoid_: safe move, game-over check

**Solve**:
The outcome of searching one game: won or lost, with the move list and the number of placements evaluated.
_Avoid_: run, attempt, trial

## Scoring

**Evaluator**:
A function from board to integer score. Swappable: the ablation compares implementations on identical games.
_Avoid_: heuristic, fitness, objective

**Feature**:
One integer quantity an evaluator measures about a board — height, holes, transitions, wells.
_Avoid_: metric, term, weight

**Weight**:
A scaled integer multiplier applied to a feature. Fixed point, not floating point, so native and wasm scores agree exactly.
_Avoid_: coefficient, parameter

**Evaluation**:
One scored placement. Throughput is quoted in evaluations per second.
_Avoid_: node, sample, iteration
