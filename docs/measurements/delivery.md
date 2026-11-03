# Delivery: the sizes the deploy ships

Generated for ticket 17 from the built tree, `web/dist`, which is the directory
Wrangler uploads, and not from a live URL. No deploy has been run on this account
for the Lab yet: nothing was logged into, no token exists and nothing was pushed.
The transferred size *over the edge* is therefore unmeasured. What is below is the
number §9.3's budget is asserted against, summed from the bytes that get
uploaded.

## How to regenerate

```bash
cd web
npm run build                              # wasm + page + dist/manifest.json
node scripts/gate-artifacts.mjs sizes      # the per-file table below
```

`gate-artifacts.mjs sizes` is the same measurement gate 6 asserts, so this file and
the gate cannot disagree. It gzips `dist/**/*.{html,js,css,wasm}` with Node's
`gzipSync` at the default level, and leaves source maps out because they are not
the payload.

## §12's rows, measured

The spec's §12 lists the numbers the build owes back. Every row below carries its
number and the machine that produced it; the one row that cannot have a number yet
is the deployed commit's gate results, because nothing is deployed (the sequence is
in `docs/runbook.md`).

| §12 row | Number | Machine label | Artifact |
| --- | --- | --- | --- |
| `DEFAULT_BEAM` | **25**, named on cost: `default_beam_basis: "cost"`, `quality_axis: "no-resolution"` | AMD Ryzen 7 5700X (8C/16T), 31.9 GB, Windows 11 Pro; wasm through node 24.21.0 (V8); rustc 1.99.1 release, `lto=fat, codegen-units=1` | `data/measurements/width-curve.json` |
| Width cap (the slider maximum) | **800**, `cap_bound: "budget"`. 1,600 was measured and failed the 200 ms default-config p90 budget at **203.46 ms** native (≈215 ms extrapolated on wasm) | same | `data/measurements/width-curve.json` (`overshoot`) |
| Shipped evaluator and weight table | **E1 / `E1_PUBLISHED`**: Dellacherie–Thiery, 8 features; dedup and no-suicide unconditional, the Tail switchable | deterministic; the artifact records the box | `data/measurements/ablation-ladder.json`; weights in `engine/src/evaluator.rs` |
| Reference run rows | index **3** (live), both configurations: **1,000/1,000 wins**, wasm **4bfa56b5…** (58,852 B), commit `a773ce5`, 2026-11-03T03:37:10Z. Index 2 is void and says so | each row carries cpu, os, runtime, rustc and node | `data/dataset/*/reference.jsonl` |
| wasm raw size and whole-page gzip | **58,852 B** raw; **77,495 B** gzip over **234,046 B** raw, 5 files (source maps excluded) | same box; rustc 1.99.1 win-gnu with `strip = true`; Node 24.21.0 `gzipSync` default level, Vite 7.3.7 | gate 6; the per-file table below |
| Solve p90, default configuration | **4.536 ms** at beam 25, wasm (budget 200 ms) | Reference row index 3's machine | the `goal8-p40-cap7` reference row's `stats` |
| Solve p90, harder configuration | **6.547 ms** at beam 25, wasm (budget 1,000 ms) | same | the `goal12-p100-cap7` reference row's `stats` |
| First solve p75 | **5.824 ms** over 50 trials, covering compile, instantiate, `gen_game` and `solve`, with network fetch and Worker spawn excluded (budget 1,000 ms) | same box; node 24.21.0 V8 | `node bench/tools/first-solve.mjs --trials 50 --json` |
| Run wall time | **1.73 s** click-to-last-game-applied (the script's own budget check, 5 s), and **12.3 s** as the page's Run readout prints it for the same 2,000 games on 8 workers, because the readout includes Worker start-up. §9.3's budget is written against a Run; the script's budget check is the one that gates, and the readout is recorded beside it rather than adjusted. Against the deployed URL the same two numbers are **1.96 s** and **13.2 s** | headless Chrome on the same box, 2026-11-03, locally and against `https://lab.samibentaleb.com/` | `node web/scripts/browser-check.mjs` |
| The six gate results on the deployed commit | **all six PASS** on `b76cfdf`, gate 6 = 9 checks, 0 skipped (CI run 41682733864). The deployed artifact is that build: wasm `4bfa56b5…`-era `engine.wasm`, 58,852 B, uploaded as Worker version `2dc6de11-eefa-46ed-91a8-61adeca51af4` | the CI runner (`windows-2025`) | `.github/workflows/gates.yml`; `npx wrangler deployments list` |
| A frozen trace fixture | `web/test/fixtures/trace-seed0-goal8-p40-cap7-band25.json`: 444 records over 20 Layers at band 25 | `solve_traced` on the default game | the committed fixture; regenerate with `web/scripts/gen-trace-fixture.mjs` |

Per file, so a regression is attributable:

| file | raw | gzip |
| --- | --- | --- |
| `assets/engine-BtPiaFvn.wasm` | 58 852 | 21 737 |
| `assets/index-CgWIuYq_.js` | 121 140 | 41 487 |
| `assets/index-DuOtfHqX.css` | 25 252 | 5 702 |
| `index.html` | 22 351 | 5 908 |
| `assets/solve.worker-Bc5ui2l-.js` | 6 451 | 2 661 |
| **total** | **234 046** | **77 495** |

Against §9.3's budgets: the wasm is 57.5 KiB against a 100 KiB warn line and a
150 KiB fail line; the page is 75.7 KiB against a 150 KiB fail line. Both pass, and
gate 6 asserts them on every push.

## Reproducibility, measured

Two consecutive `npm run build` runs of the same tree produced **byte-identical**
`dist/`: all nine files (including `manifest.json`, `_headers` and the two source
maps) with identical `sha256` sums and identical hashed filenames. That is the "a
rebuild that changes nothing produces the same hashes" clause, verified on this box
with this toolchain.

The same has to hold on `windows-2025`, where the deploy runs. The pin
(`rust-toolchain.toml`) is committed and the workflow installs from it, but the
runner's rustc build is the image's, so exact cross-machine reproducibility is
**unverified**. That is also why the deploy ships the artifact `gates.yml` produced
instead of rebuilding: the question never has to be answered.

## Observed locally, and not observed

Observed, on this box, with no network and no credentials:

- `npx wrangler deploy --dry-run` (wrangler 4.156.1, offline) exits 0, reports "Read 10
  files from the assets directory" and `--dry-run: exiting now`: the asset set and
  the config key set parse, and nothing was uploaded;
- `npx wrangler dev` serving `dist/` with `web/wrangler.jsonc`, then
  `bash web/scripts/verify.sh --url http://127.0.0.1:…` → **PASS 45 checks**:
  `application/wasm`, `public, max-age=31536000, immutable` (exactly, not
  comma-joined), the page's `max-age=0, must-revalidate`, an ETag, `nosniff`, every
  manifest path resolving, the fallback body identical to `/`, and the deployed
  module's `abi_version = 2` with `alloc`/`gen_game`/`solve` present;
- `node web/scripts/browser-check.mjs` → **PASS**, including the two deploy-time
  assertions: `worker 1 · abi 2 · 40 pieces` (the page's own ABI handshake) and the
  origin check;
- the built `dist/` structure and reference chain: `index.html` names exactly the
  hashed entry chunk and stylesheet, and the entry chunk names the hashed wasm and
  worker chunk;
- the two sizes above, and the two-build hash reproducibility.

**Observed against the live edge, 2026-11-03.** `wrangler deploy` from `web/` at
`b76cfdf` created Worker `tetris-lab`, version
`2dc6de11-eefa-46ed-91a8-61adeca51af4`, uploaded 8 assets, and attached
`lab.samibentaleb.com` on the first try — no dashboard step. Total upload 0.35 KiB
(0.25 KiB gzip); the assets themselves are served from the edge cache.

- `bash web/scripts/verify.sh --url https://lab.samibentaleb.com` → **PASS 45
  checks**: the document's headers and `must-revalidate`, the wasm's
  `application/wasm` and `public, max-age=31536000, immutable`, a live `ETag`, the
  `7 assets, same hashes` identity check, the SPA fallback (200 `text/html`
  identical to `/`), `/index.html` answering 307 to `/` at the edge, and the
  deployed module's `abi_version = 2` with `alloc`/`gen_game`/`solve` present.
- `node web/scripts/browser-check.mjs --url https://lab.samibentaleb.com/` →
  **PASS**, Run 2,000/2,000 games, 1.96 s click-to-last-game (5 s budget).
- Edge `content-encoding: zstd` and `cf-cache-status: HIT` were read from the same
  responses.
- The deploy was **manual**. `.github/workflows/deploy.yml` has still never run on
  a runner, so the CI-deploy path, its concurrency group and its preview input are
  unexercised; the next green push to `main` is what tests them.

**Still not observed.** No claim in this repository should imply otherwise:

1. **The preview URL.** `wrangler versions upload --preview-alias` has never run, so
   no preview URL exists and the dispatch path is unexercised.
2. **Rollback.** Never exercised. The command, the 100-version window and the
   `<version-id>` argument come from `wrangler rollback --help` and the docs.
3. **A scoped API token.** The deploy used the interactive OAuth token, which
   carries `workers_routes (write)`. Whether a `Workers Scripts:Edit` API token can
   also attach a *new* custom domain on a first deploy is still **unverified**.
4. **Render retirement.** The old host has not been torn down. The URL is served by
   `tetris-lab`, and the old service's hostname is untouched; runbook step 3 is the
   one that proves it dead.
5. **The probe Worker.** `browser-tetris-lab-probe` still exists; it no longer fronts
   `lab.samibentaleb.com`, but nothing has deleted it.
6. **The repository rename, the `main` merge and branch protection.** Runbook steps
   4–6, none run.
7. **The Worker's request count under Workers Free.** Static-asset requests are
   documented as free and unmetered, so the 100 k/day cap should be inert; no live
   request was counted.
