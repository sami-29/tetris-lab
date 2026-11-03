# Tetris Lab: the owner's guide

This is the file to read if you own the repository and did not write it. It says
what the Lab is, where each part lives, which numbers are load-bearing, what has
been settled, what is still open, and what only you can do. The developer entry
is [`README.md`](../README.md); the vocabulary is [`GLOSSARY.md`](../GLOSSARY.md).

## What it does

A paragraph you can send as it stands:

> Tetris Lab is a website that solves a Tetris line-clearing game in the visitor's
> browser. You give it a 20×10 board and a known sequence of pieces, and a Rust
> engine compiled to WebAssembly searches for an order of placements that clears a
> target number of lines, then plays its answer back on screen. The same engine
> also solves the 2,000 committed Dataset games inside the tab and shows the
> visitor's own result beside a Reference run that was measured on known hardware
> and committed to the repository. There is no server compute, no database and no
> account: the site is static files on Cloudflare, and every number on the page
> comes out of the wasm build in this tree.

The site explains the search itself, in a drawer called the Explainer, and
`web/src/ui/explainer.ts` is the text of that drawer. This guide does not repeat
it.

## Who it is for

A visitor arrives at one page that opens on a game solving itself, can change
that game (seed, goal, piece count, starting height), watch the beam search as a
score band with the winning line drawn through it, drill into a bounded tree, run
the whole Dataset, and read the Explainer. The page does not scroll: it is five
regions and a drawer, wired in `web/src/main.ts`. It is built for a desktop
screen.

The owner needs something different: which part of the tree is the source of
truth for which claim, which numbers a visitor is being compared against, which
changes are ordinary and which move a committed golden value, and what is left
that needs a human with credentials.

## The one live URL

https://lab.samibentaleb.com is served as the Cloudflare Worker `tetris-lab`
from static assets only: `web/wrangler.jsonc` has no `main` and no `build` key,
so there is no Worker script and no Cloudflare-side build. `deploy.yml` deploys
it after a green `gates` run on `main`, and uploads the exact `web/dist` tree that
run measured, so there is no second build (`README.md`).

It is live, and it went up on 2026-11-03 as Worker version
`2dc6de11-eefa-46ed-91a8-61adeca51af4`, deployed by hand from `b76cfdf` on
`rewrite` and verified against the custom domain: `verify.sh` PASS 45 checks, the
browser check PASS with the Run at 1.96 s. From the first green push to `main`,
`deploy.yml` owns the deploy instead. The probe Worker `browser-tetris-lab-probe`
still exists but no longer serves the domain; the teardown, the repo rename, the
`main` replacement merge and branch protection are steps 2–7 of
[`docs/runbook.md`](runbook.md), and none of them has run.

## The map

| Path | What it owns |
| --- | --- |
| `engine/` | The search and the scoring, and the one generator. Pure Rust: board, pieces, beam, dedup, tail, evaluators, trace. No I/O, no wasm types, no JS. |
| `engine-wasm/` | The boundary: six raw exports at `abi_version` 2, the static buffer offsets and the packed result. No search and no scoring logic. |
| `bench/` | The Dataset and every number derived from it: verify, fingerprint, export, the Reference run, the width curve, the ablation ladder, tuning, the six gates. The only crate that reads or writes `data/dataset/`. |
| `web/` | The page. Vite, vanilla TypeScript, Canvas 2D, zero runtime dependencies. `web/src/core/` is the only code that touches `WebAssembly`; `web/src/ui/` is the shell and every view; `web/src/workers/` the worker entry. |
| `data/dataset/` | Two configurations of 2,000 games, each with a manifest and an append-only `reference.jsonl`. The rules for touching it are in `data/dataset/README.md`. |
| `data/measurements/` | The width curve, the ladder, the tuning record and the golden evaluations fingerprint, as JSON a build step reads. |
| `docs/adr/` | The three settled decisions, with what was rejected and why. |
| `docs/research/` | The surveys and benchmarks the ADRs rest on. |
| `docs/measurements/` | The written-up measurements: the delivery sizes, the ablation write-up. |
| `docs/design/` | The two prototype artifacts the UI was judged against. Frozen, not part of the build. |
| `docs/runbook.md` | The cutover steps, which only a maintainer can run. |

The split between the four crates is the part worth keeping in your head.
`engine/` owns the answer: what a legal placement is, how wide the beam is, what
dedup keeps, how the last three pieces are searched, what a score is, and what a
generator makes of a seed. `engine-wasm/` owns the boundary and nothing else.
`bench/` owns truth: the committed games, the gates that keep them honest, and
every measurement. `web/` owns presentation: what a visitor sees and what a URL
means. `engine/` never imports wasm or JS types and `engine-wasm/` holds no search
code, so a number the browser reports can be required to equal the one the native
bench reports (`README.md`, `docs/adr/0002-engine-architecture.md`).

So a change that moves a number the page prints is an `engine/` change and lands
as a gate diff; a change that moves what the page says is a `web/` change.

## Build, test, gate

The toolchain is the committed pin in `rust-toolchain.toml`, not the word
"stable": a moving channel would let a Rust upgrade move the Dataset fingerprint
(`README.md`). On this box that pin is a windows-gnu host toolchain with the
`wasm32-unknown-unknown` target, because the box has no MSVC linker.

```bash
npm --prefix web ci                 # vite and typescript only; no runtime dependencies
npm --prefix web run build:wasm     # the wasm artifact  ->  web/pkg/
npm --prefix web run build          # wasm + generated facts + tsc + vite + manifest
npm --prefix web run dev            # dev server
npm --prefix web run gates          # build, banned words, cargo test, node tests, the six gates
cargo fmt --all --check             # CI runs this first; a diff fails the run
```

`npm --prefix web run gates` is the same set of checks CI runs, and it is the
command to trust before a push. `cargo fmt --all --check` is separate, and CI runs
it before anything else.

The native bench is where the Dataset and the measurements are driven:

```bash
cargo run -p bench --release -- verify         # gate 1 alone: every game regenerated and compared
cargo run -p bench --release -- gate all       # the six gates, 1-6
cargo run -p bench --release -- show --config goal8-p40-cap7 --seed 0
cargo run -p bench --release -- export --accept-change "one-line reason"
```

Exit codes are part of the interface: `0` clean, `1` Dataset drift or a failed
gate, `2` usage, I/O, or a missing prerequisite such as a built `web/pkg/`
(`README.md`).

The six gates are the generator fingerprint, manifest agreement, replay validity,
native/wasm parity on 100 fixed seeds from the test split, determinism, and the
budgets. Those budgets are the wasm's raw size (warn above 100 KB, fail above
150 KB), the whole page's gzipped size (fail above 150 KB) and the
evaluations-to-solve fingerprint against committed golden values with a loose
wall-clock bound around it (`README.md`). Gate 6 also prints the sizes it
measured, so the two size numbers below and the gate cannot disagree.

For the owner, "green" has a single meaning: a `gates` run. `gates.yml` runs it on
every push and pull request, `deploy.yml` triggers only on a completed successful
`gates` run for a push on `main`, and `main` is meant to require the `gates` check
(runbook step 6).

Two test suites exist: the Rust workspace and the node tests under `web/test/`,
which read the source, the committed fixtures and the generated artifacts. The
built page is covered separately, by `npm --prefix web run check:words` inside
`gates` and by `web/scripts/browser-check.mjs` against a URL.
`bench/tests/cutover.rs` is worth knowing about on its own: it fails if a
committed document cites a path a fresh clone does not have, so a document that
lies about the tree breaks the build. This file is subject to it.

## What is measured, and where the numbers live

Four groups of numbers, each with one home.

### The build's size

Gate 6 measures it and `docs/measurements/delivery.md` writes it down, with a
per-file table. The current build is 58,852 B of wasm raw and 21,737 B gzipped,
and a whole page of 77,495 B gzipped over 234,046 B raw across 5 files. Against
the budgets that is 57.5 KiB of wasm against a 100 KiB warn line and a
150 KiB fail line, and a 75.7 KiB page against a 150 KiB fail line. Regenerate the
table with `node web/scripts/gate-artifacts.mjs sizes`, which is the same
measurement gate 6 asserts.

### The quality and cost of the search

`data/measurements/width-curve.json` holds the width curve, which names
`DEFAULT_BEAM` 25 and the width cap 800, and records that 1,600 was measured and
failed the 200 ms default-configuration budget at 203.46 ms. `data/measurements/ablation-ladder.json` and
`docs/measurements/ablation-ladder.md` hold the ladder that chose the shipped
evaluator: E1 at published weights, with dedup-off and no-suicide-off deleted from
the engine because neither moved anything beyond its confidence interval, and the
tail search kept because it did. `data/measurements/golden-evals.json` holds the
evaluations-to-solve fingerprint gate 6 compares against.

### What a visitor is compared against

`reference.jsonl` in each configuration directory, append-only, one labelled
measurement per row. Index 3 is the live row for both configurations: 1,000 of 1,000 wins, wasm `4bfa56b5…` at 58,852 B, engine
commit `a773ce5`, measured 2026-11-03T03:37:10Z. From the same rows: Solve p90
4.536 ms on the default configuration against a 200 ms budget, 6.547 ms on the
harder one against a 1,000 ms budget, and first solve p75 5.824 ms over 50 trials
against a 1,000 ms budget. Index 2 is void and says so in its own `reason` field;
old rows are never edited, only superseded (`docs/measurements/delivery.md`,
`data/dataset/README.md`).

### The Dataset itself

Two configurations, 2,000 seeded games each, split into a test split (seeds
0-999, the only split a Run or a Reference run sees) and a tuning split
(1,000-1,999, touched only by tuning, ablations and width curves).
`games.txt` is 270,949 B for the goal-8 configuration and 390,951 B for the goal-12
one, and 926 of the 2,000 cap-7 boards are shorter than the cap
(`data/dataset/README.md`). The trace fixture the band and the tree render from is
`web/test/fixtures/trace-seed0-goal8-p40-cap7-band25.json`: 444 records over 20
layers at band 25 (`docs/measurements/delivery.md`).

The rule that ties these together: a number on screen is a labelled figure with a
provenance, and it is either the engine's, the trace's, a replay's, the URL's, a
committed artifact's, or the page's own clock (`web/src/main.ts`).

## The decisions that are settled

Three ADRs, each with the options it rejected and the measurement behind the
rejection.

`docs/adr/0001-rust-wasm32-scalar-bitboards.md` settles the language and the
representation: Rust stable on `wasm32-unknown-unknown`, raw `WebAssembly` exports
instead of `wasm-bindgen`, ten u32 column words per board, scalar code, one
instance per worker. The measurements are in
`docs/research/06-bitboard-benchmark.md`: 16.4M evaluations per second in wasm
against 8.6k for the numpy solver, u64 columns 2.1× slower in wasm, SIMD128 at
0.85× scalar, C++ at 21.5M native against Rust's 27.7M with `-march=native` and
13.4M against 17.7M on default flags, so the 15% win that would have justified a
C++ toolchain never appeared.

`docs/adr/0002-engine-architecture.md` settles the search: a layered beam over
bitboards with per-layer dedup on the board alone, a no-suicide rule, an exact
three-piece tail search, and the evaluator behind a trait. It also settles the
things that are hard to notice later: scoring is fixed point so native and wasm
agree exactly, there is no RNG, and the visit order and the tie-break (insertion
order) are part of the contract. A trap is defined as "respawn the worker and
report it", which costs about 1.6 ms.

`docs/adr/0003-generator-contract.md` settles the generator: splitmix64 with Lemire
bounded draws, two streams separated by purpose so raising the starting height
keeps the pieces, a pre-fill that stops before the crossing piece, and the S/Z
re-shuffle moved to the bag seam. The pre-fill clears completed lines, which is
invisible in review and changes 16 of 5,000 measured games at cap 7.

Then the numbers that a measurement, not a preference, settled: E1 ships
(`docs/measurements/ablation-ladder.md`), `DEFAULT_BEAM` is 25 and the width cap is
800 (`data/measurements/width-curve.json`). Both are read by the page at build
time, so a table on the site, the URL default and the slider maximum cannot drift
apart.

Words are settled too, and `GLOSSARY.md` is the authority. Lab, Run, Dataset,
Split, Reference run, Playground, Explainer, Gate, Board, Column word, Seed,
Generator, Sequence, Goal, Placement, Move, Beam width, Layer, Dedup, Tail search,
No-suicide rule, Solve, Evaluator, Feature, Weight, Evaluation. Each entry lists
the near-synonyms that must not stand in for it, and a gate enforces part of that
list on the built page (`web/scripts/words.mjs`).

## What only the maintainer can do

[`docs/runbook.md`](runbook.md) holds it, in seven numbered steps, each with the
command where one exists and a close condition that is not a feeling. In order: deploy
the Lab before moving the domain, delete the probe Worker, retire the previous
host, rename the repository, merge `rewrite` into `main` as one replacement
commit, protect `main` with the `gates` check, and write the numbers back into the
release note.

The rule behind it is simple: no push, no deploy, no repository rename, no domain
change and no merge into `main` happen from an automated run. Everything there
needs a credential or a dashboard, which is why it is the one part of this effort
nothing has touched.

## Known overruns and open edges

### The Run readout is over its budget

The Run's budget is 5 s (`docs/runbook.md`). The page's own readout for 2,000
games on 8 workers reads 12.3 s, because it includes Worker start-up; the browser check's own measurement,
from the click to the last game applied, is 1.73 s and is the number the script's
budget assertion is written against. Both are recorded in
`docs/measurements/delivery.md` and neither was adjusted, and the runbook says
plainly not to compare the readout against the script's budget. If you want the
two measures to agree, the readout is what changes.

### The width curve did not run to the top

`data/measurements/width-curve.json` records `complete: false` with widths 1,600,
3,200 and 6,400 unmeasured, because the curve stops at the first width that fails a budget and 1,600 failed the 200 ms
default-configuration budget at 203.46 ms on native. The cap is therefore 800, and
`DEFAULT_BEAM` was named on cost rather than on quality: both shipped
configurations win 1,000 of 1,000 games at width 800, so no width could be
separated on the quality axis at all.

### What the deploy settled, and what it did not

The six gate results on the deployed commit are measured (`b76cfdf`, CI run
41682733864, all six PASS), and so is the live smoke test (`verify.sh` 45 checks,
browser check PASS, 1.96 s). `docs/measurements/delivery.md` carries both numbers
with their machine label rather than blanks.

`deploy.yml` has still never run on a runner: this deploy was manual, so the CI
deploy path, its concurrency group and its preview input are unexercised. The
preview URL and rollback are equally unexercised, and the Render name and the probe
Worker are still standing. Those are listed, with the rest, under "Still not
observed" in `docs/measurements/delivery.md`.

### Cross-machine rebuilds are unverified

Two builds of the same tree on this box produced byte-identical output across all
nine files (`docs/measurements/delivery.md`), and the same has to hold on the CI
image, which it has not been checked against. The deploy ships the artifact the gates produced
instead of rebuilding, so the question does not have to be answered for a deploy
to be correct.

## Fragile: what breaks easily

The generator is the sharpest edge. A changed constant, draw order or bound moves
every committed board, and gate 1 fails naming the first game that moved. The only
legitimate way through is the archive-and-append procedure in
`data/dataset/README.md`: re-export with `--accept-change` and a one-line reason,
which archives the old file under its own fingerprint, bumps `generator_version`
and records the reason, then append a new Reference row and commit all of it
together. Never hand-edit `games.txt`, never `git add -f` a Dataset file, never add
a `.gitignore` pattern under `data/dataset/`.

Determinism is fragile in the same way and less visibly. There is no RNG in the
engine, ties break by insertion order and dedup keeps the state with more lines
cleared, so reordering a loop or changing a comparison changes results and must be
a deliberate golden update (`docs/adr/0002-engine-architecture.md`).
`web/src/generated/engine-facts.ts` is generated, committed, and embeds the engine
commit, so every merge dirties it; re-run `npm --prefix web run build:facts`, and
read its diff as a measurement's diff, never a hand edit (`README.md`).

The deploy has two known traps. The smoke test fails the run and never rolls back,
so a live-but-wrong deploy is your call; the revert is `npx wrangler rollback`
against a numbered version, and `_headers` rides in the version payload so the
headers come back with the assets (`README.md`). And the two repository secrets,
`CLOUDFLARE_API_TOKEN` scoped to Workers Scripts on the one account and
`CLOUDFLARE_ACCOUNT_ID`, have never been created, so the first deploy is also the
first test of them.

The bounded tree is readable only because everything off the selection dims. At
width 200 there is nothing to explore sideways: the beam keeps 200 states out of
roughly 3,400 to 6,800 candidates per layer, the mean number of children per
survivor is 1.0, and six states a third of the way in feed the whole final layer
(`docs/design/t14-tree/FINDINGS.md`). A UI change that removes the dimming or adds
a "follow the leader" affordance makes that artifact unreadable again.

## Handing the repo to someone else

What they need besides the clone: the Cloudflare account that holds the
`samibentaleb.com` zone, the two repository secrets above, and a GitHub repository
that still carries the old name until runbook step 4 renames it.

What they will not get: the design spec and the ticket history. Both are the
operator's local working notes, ignored by `.gitignore` and deliberately not part
of the repository. The durable record is the three ADRs, the research notes under
`docs/research/`, the measurements under `docs/measurements/` and the Reference
rows, and `README.md` says so. The design artifacts under `docs/design/` travelled
with the cutover, and they are frozen evidence, not part of the build.

What is true on handover today: the Lab is built and green on `rewrite`, `main`
still holds the application it replaces, and the Lab's URL serves the probe. The
runbook is the sequence that closes the gap, in the order that avoids a window
where the URL answers nothing.

What to tell them to distrust: the git history. The application this replaces is
recoverable from it (`git log -- app.py`), and `docs/research/06` names
`4bb9354` as the commit the deleted benchmark harness and the trace generator can
be recovered from, but none of that is evidence for the Lab. Every claim about the
Lab's behaviour is behind a gate.

A sensible reading order: `README.md` for the developer entry, `GLOSSARY.md` for
the words, the ADR that touches the area you are about to change,
`docs/measurements/delivery.md` for the numbers, then `docs/runbook.md` for what
has not run.

## How to explain it

The 30-second version: it is a page that solves Tetris boards in your browser. One
Rust engine, compiled to WebAssembly, does the search; the page shows the search
as a band of states with the winning line drawn through it, and plays the result
back. The same engine solves 2,000 committed games in your tab in 1.73 s, measured
from the click to the last game applied on the reference box, beside a reference
run that was measured on known hardware. The page is static files on Cloudflare,
so nothing about a visit reaches a server we run.

The 3-minute version adds the machinery. The engine searches a known piece
sequence with a beam: it keeps the best 25 states per layer, removes duplicate
boards inside a layer, refuses a placement that ends the game unless nothing else
is legal, and searches the last three pieces exactly. Placement quality is a
scoring function of the board with eight integer features at fixed-point weights,
chosen by the ablation ladder rather than by preference. The games come from a
committed Dataset of two configurations, each with a test split that a Run and a
Reference run both measure, all generated by one deterministic function whose
fingerprint every push recomputes. Six gates block a build on the fingerprint, the
manifests, replay validity, native/wasm parity, determinism and the size and
evaluation budgets, and the deploy only ever ships what a green gate run measured.
