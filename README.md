# Tetris Lab

A browser lab for one question: given a 20×10 board and a known Tetris piece
sequence, can the search clear a target number of lines — and how? One Rust
engine, compiled to `wasm32-unknown-unknown`, solved inside a visitor's tab. No
server compute, no database, no accounts, nothing stored: everything on the page
came out of the wasm build in this repository.

**Live:** https://lab.samibentaleb.com, deployed from `main` by
`.github/workflows/deploy.yml` after a green gate run, as the Cloudflare Worker
`tetris-lab` (static assets only, no Worker script). The steps that first put it
there are the maintainer's, in [`docs/runbook.md`](docs/runbook.md); until they
have run, the URL still answers with the ticket-09 probe.

What the engine does is explained **on the site itself** (the Explainer drawer),
for the engine that actually ships. This README is the developer entry.

## What is here

```text
engine/            pure Rust: board, pieces, generator, beam, dedup, tail,
                   evaluators, trace. No I/O, no wasm types, no JS; the only
                   implementation of the search, host-testable.
engine-wasm/        the exported ABI and nothing else (six raw exports, abi 2).
bench/             native binary: Dataset verify/export/fingerprint, the Reference
                   run, the width curve, the ablation ladder, tuning, the gates.
                   Reuses engine/ exactly as the wasm build does.
web/               Vite + vanilla TypeScript + Canvas 2D, zero runtime dependencies.
  src/core/        the only code that touches WebAssembly
  src/workers/     the worker entry and the message protocol
  src/ui/          the workbench shell, board, band, tree, inspector, Explainer
  scripts/         build-wasm.mjs, build-facts.mjs, manifest.mjs, verify.sh,
                   browser-check.mjs, check-words.mjs, the gate helpers
data/dataset/      the committed Dataset, two configurations × 2,000 seeds —
                   see data/dataset/README.md before touching it
data/measurements/ the committed width curve, ladder, tuning record and golden
docs/adr/          0001–0003: the decisions, with their measurements
docs/research/     01–06: the surveys and benchmarks those decisions rest on
docs/design/       the two prototype artifacts the UI was judged against
docs/measurements/ the delivery sizes and the ablation write-up
docs/runbook.md    the cutover steps only a maintainer can run
.github/workflows/ gates.yml (every push), deploy.yml (a green gates run on main)
```

`engine/` never imports wasm or JS types; `engine-wasm/` never contains search or
scoring logic. That split is what makes "the browser number is not a guess"
structural, and it is the first thing to preserve when changing any of it.

## Build the engine

The toolchain is the committed pin in `rust-toolchain.toml`, not the word
"stable": a moving channel would let a Rust upgrade move the Dataset's
fingerprint. This box needs the GNU host toolchain (there is no MSVC linker), so
the pin names it; CI installs the same pin on `windows-latest`.

```bash
rustup toolchain install 1.99.1-x86_64-pc-windows-gnu --target wasm32-unknown-unknown
npm --prefix web ci                 # vite + typescript only; no runtime dependencies
npm --prefix web run build:wasm     # cargo build --release --target wasm32-unknown-unknown
                                    #   -p engine-wasm  →  web/pkg/
npm --prefix web run build          # build:wasm + tsc --noEmit + vite build  →  web/dist
npm --prefix web run dev            # vite dev server
```

`web/src/generated/engine-facts.ts` is generated from the committed artifacts by
`npm run build:facts` (the build and the tests both run it first) and is
committed, so a fresh clone type-checks before anything is generated. If it shows
as dirty, re-run the command: its diff is a measurement's diff, never a hand edit.

## Run the native bench

`bench` is the only crate that reads or writes `data/dataset/`. Its verbs:

```bash
cargo run -p bench --release -- verify       # gate 1: regenerate every game, compare bits and fingerprint
cargo run -p bench --release -- fingerprint  # <config> <fingerprint> <path>; reads no manifest
cargo run -p bench --release -- show --config goal8-p40-cap7 --seed 0
cargo run -p bench --release -- gate all     # the six gates, 1–6
cargo run -p bench --release -- reference --append --reason "…"
cargo run -p bench --release -- width-curve --summarise data/measurements/width-curve.json
```

Exit codes are part of the interface: `0` clean, `1` Dataset drift, a failed gate
or a rejected tuning run, `2` usage, I/O, or a gate whose prerequisite (a built
`web/pkg/` or `web/dist/`) is missing.

## Export the Dataset

The Dataset is two committed configurations of 2,000 seeded games. Never edit it
by hand and never `git add -f` it:

```bash
cargo run -p bench --release -- export --accept-change "<one-line reason>"
```

The exporter archives the old file under its fingerprint, bumps
`generator_version` and refuses to write without a reason. The full procedure,
the review checklist and the sizes are in
[`data/dataset/README.md`](data/dataset/README.md).

## Run the gates

```bash
npm --prefix web run gates
# = npm run build && npm run check:words && cargo test --workspace
#   && node --test "test/**/*.test.mjs" && cargo run --release -p bench -- gate all
cargo fmt --all --check   # CI runs this first; a diff fails the run
```

The six gates are: (1) the generator fingerprint, (2) manifest agreement, (3)
replay validity, (4) native/wasm parity, (5) determinism, (6) the budgets — the
wasm's raw size, the whole page's gzipped size, and the evaluations-to-solve
fingerprint. The budgets are 150 KiB wasm (warn at 100 KiB) and 150 KiB gzipped
page; the current numbers are printed by the gate on every run. CI runs the same
commands on every push and pull request, and the deploy only ever runs after a
green one.

## Deploy

```bash
cd web && npm ci && npm run build
node scripts/manifest.mjs        # dist/manifest.json: logical name → hashed path
npx wrangler deploy              # one numbered Worker version: assets + headers atomically
bash scripts/verify.sh --url https://lab.samibentaleb.com
node scripts/browser-check.mjs --url https://lab.samibentaleb.com/
```

A push to `main` deploys through `.github/workflows/deploy.yml`; it triggers on a
completed `gates` run (`workflow_run`, filtered to a successful push on `main`)
and uploads the exact `web/dist` tree the gates measured — there is no second
build. A hand-triggered run (`workflow_dispatch`, inputs `ref` and `alias`)
uploads a *preview version* with `wrangler versions upload --preview-alias`; it
changes no deployment.

### The two secrets

Repository secrets, and nothing else:

| Secret | Value |
| --- | --- |
| `CLOUDFLARE_API_TOKEN` | an API token scoped to **Workers Scripts:Edit** on the one account |
| `CLOUDFLARE_ACCOUNT_ID` | that account's id |

No zone, DNS or KV permission is in the token: the deploy uploads a version and
attaches a domain that is already on the account. The first attach of
`lab.samibentaleb.com` to a *new* Worker may need a one-time step in the dashboard
(or a logged-in `wrangler deploy`) — a token limited to Workers Scripts has not
been proven sufficient for that, and pretending otherwise is how a first deploy
fails at 02:00.

### After a deploy

The workflow runs the post-deploy smoke test against the live URL and fails loudly
on any mismatch. It **never rolls back** — a live-but-wrong deploy is the
operator's call, and a rollback to a known-bad version is worse than a red badge.
The version id is in the run log (`wrangler deployments list`), so reverting is one
command:

```bash
cd web
npx wrangler deployments list          # the numbered versions, newest first
npx wrangler rollback <version-id> -m "why, in one line"
```

`_headers` is part of a version's payload, so a rollback restores the headers with
the assets — the page cannot end up pointing at a hashed file that is not there.
To run the same two checks by hand against any URL (including a preview), see the
block above; `scripts/verify.sh` reads the manifest that `npm run build` writes, so
build before pointing it at a URL. What was verified locally, and what was not
observed, is in [`docs/measurements/delivery.md`](docs/measurements/delivery.md).

## Where the decisions live

- `docs/adr/` — `docs/adr/0001` (Rust on wasm32, scalar u32 column
  bitboards, raw exports), `docs/adr/0002` (the engine's architecture),
  `docs/adr/0003` (the generator contract). Each records what was measured, what
  was rejected and why. Read the ADR that touches the area you are about to change.
- `docs/research/01..06` — the surveys and benchmarks the ADRs cite: language and
  toolchain, the host, the browser compute ceiling, the algorithm frontier, the
  app architecture, and the bitboard benchmark that decided ADR 0001.
- `docs/design/` — the two prototype artifacts the UI was judged against
  (`t10-mockup.html`, `t14-tree/`). Frozen; not part of the build.
- `GLOSSARY.md` — the vocabulary the site and the code use, with the
  near-synonyms each term replaces. Vocabulary: the glossary; decisions: the
  ADRs.
- **What the engine does is explained on the site itself** (the Explainer
  drawer), and `web/src/ui/explainer.ts` is its source. This README does not
  repeat it.
- The ticket history and the design spec are not in this repository. The ADRs and
  the research notes are the durable record, and the Flask application this
  replaced is recoverable from git history only: `git log --oneline -- app.py`.

## Licence

MIT.
