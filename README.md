# Tetris Lab

A browser lab for one question: given a 20×10 board and a known Tetris piece
sequence, can the search clear a target number of lines, and how? One Rust engine,
compiled to `wasm32-unknown-unknown`, solved inside a visitor's tab. There is no
server compute, no database and no account: everything on the page came out of the
wasm build in this repository.

**Live:** https://lab.samibentaleb.com, served by the Cloudflare Worker
`tetris-lab` as static assets only (no Worker script). It went up on 2026-11-03 as
version `2dc6de11-eefa-46ed-91a8-61adeca51af4`, deployed by hand from `rewrite`;
from the first green push to `main`, `.github/workflows/deploy.yml` deploys it
after a gate run. The maintainer steps that remain — Render, the repo rename, the
`main` replacement merge, branch protection — are in
[`docs/runbook.md`](docs/runbook.md).

What the engine does is explained on the site itself, in the Explainer drawer, for
the engine that actually ships. This README is the developer entry.

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
  src/ui/          the Lab's shell, board, band, tree, inspector, Explainer;
                   src/ui/README.md maps it before a UI change
  scripts/         build-wasm.mjs, build-facts.mjs, manifest.mjs, verify.sh,
                   browser-check.mjs, shot.mjs (one screenshot, for looking),
                   check-words.mjs, the gate helpers
data/dataset/      the committed Dataset, two configurations × 2,000 seeds; read
                   data/dataset/README.md before touching it
data/measurements/ the committed width curve, ladder, tuning record and golden
docs/adr/          0001-0003: the decisions, with their measurements
docs/research/     01-06: the surveys and benchmarks those decisions rest on
docs/design/       the two prototype artifacts the UI was judged against
docs/measurements/ the delivery sizes and the ablation write-up
docs/runbook.md    the cutover steps only a maintainer can run
PRODUCT.md         the audience and the three things a visitor does
.github/workflows/ gates.yml (every push), deploy.yml (a green gates run on main)
.githooks/         pre-commit: the fast gate (typecheck plus the copy-pinning tests)
```

`engine/` never imports wasm or JS types; `engine-wasm/` never contains search or
scoring logic. That split is why the browser's number and the native number can be
required to agree, and it is the first thing to preserve when changing any of it.

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
as dirty, re-run the command, because its diff is a measurement's diff and you
should not edit it by hand.

## Run the native bench

`bench` is the only crate that reads or writes `data/dataset/`. Its verbs:

```bash
cargo run -p bench --release -- verify       # gate 1: regenerate every game, compare bits and fingerprint
cargo run -p bench --release -- fingerprint  # <config> <fingerprint> <path>; reads no manifest
cargo run -p bench --release -- show --config goal8-p40-cap7 --seed 0
cargo run -p bench --release -- gate all     # the six gates, 1-6
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
replay validity, (4) native/wasm parity, (5) determinism, (6) the budgets. The
budgets are the wasm's raw size, the whole page's gzipped size and the
evaluations-to-solve fingerprint, and they are 150 KiB of wasm (warn at 100 KiB)
and a 150 KiB gzipped page. The gate prints the current numbers on every run. CI
runs the same commands on every push and pull request, and the deploy only ever
runs after a green one.

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
and uploads the exact `web/dist` tree the gates measured, with no second build. A
hand-triggered run (`workflow_dispatch`, inputs `ref` and `alias`) uploads a
*preview version* with `wrangler versions upload --preview-alias`; it changes no
deployment.

### The two secrets

Repository secrets, and nothing else:

| Secret | Value |
| --- | --- |
| `CLOUDFLARE_API_TOKEN` | an API token scoped to **Workers Scripts:Edit** on the one account |
| `CLOUDFLARE_ACCOUNT_ID` | that account's id |

No zone, DNS or KV permission is in the token: the deploy uploads a version and
attaches a domain that is already on the account. The first attach of
`lab.samibentaleb.com` to a *new* Worker may need a one-time step in the dashboard
(or a logged-in `wrangler deploy`), because a token limited to Workers Scripts has
not been proven sufficient for that.

### After a deploy

The workflow runs the post-deploy smoke test against the live URL and fails loudly
on any mismatch. It never rolls back: a live-but-wrong deploy is the operator's
call, and a rollback to a known-bad version is worse than a red badge. The version
id is in the run log (`wrangler deployments list`), so reverting is one command:

```bash
cd web
npx wrangler deployments list          # the numbered versions, newest first
npx wrangler rollback <version-id> -m "why, in one line"
```

`_headers` is part of a version's payload, so a rollback restores the headers with
the assets, and the page cannot end up pointing at a hashed file that is not
there. To run the same two checks by hand against any URL (including a preview),
see the block above; `scripts/verify.sh` reads the manifest that `npm run build`
writes, so build before pointing it at a URL. What was verified locally, and what
was not observed, is in
[`docs/measurements/delivery.md`](docs/measurements/delivery.md).

## Where the decisions live

- `docs/adr/` holds `0001-rust-wasm32-scalar-bitboards.md` (Rust on wasm32, scalar
  u32 column bitboards, raw exports), `0002-engine-architecture.md` (the engine's
  architecture) and `0003-generator-contract.md` (the generator contract). Each
  records what was measured, what was rejected and why. Read the ADR that touches
  the area you are about to change.
- `docs/research/01..06` holds the surveys and benchmarks the ADRs cite: language
  and toolchain, the host, the browser compute ceiling, the algorithm frontier,
  the app architecture, and the bitboard benchmark that decided ADR 0001.
- `docs/design/` holds the two prototype artifacts the UI was judged against
  (`t10-mockup.html`, `t14-tree/`). Frozen, and not part of the build.
- `GLOSSARY.md` holds the vocabulary the site and the code use, with the
  near-synonyms each term replaces. Vocabulary belongs to the glossary; decisions
  belong to the ADRs.
- The ticket history and the design spec are not in this repository. The ADRs and
  the research notes are the durable record, and the Flask application this
  replaced is recoverable from git history only: `git log --oneline -- app.py`.

## Licence

MIT.
