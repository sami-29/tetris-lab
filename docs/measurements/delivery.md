# Delivery: the sizes the deploy ships

Generated for ticket 17 from the **built** tree — `web/dist`, the directory Wrangler
uploads — not from a live URL. The distinction matters and is not glossed over: no
deploy has been run on this account for the Lab yet (nothing was logged into, no
token exists, nothing was pushed), so the transferred size *over the edge* is
unmeasured. What is below is the number §9.3's budget is asserted against, which is
summed from the bytes that get uploaded.

## How to regenerate

```bash
cd web
npm run build                              # wasm + page + dist/manifest.json
node scripts/gate-artifacts.mjs sizes      # the per-file table below
```

`gate-artifacts.mjs sizes` is the same measurement gate 6 asserts, so this file and
the Gate cannot disagree. It gzips `dist/**/*.{html,js,css,wasm}` with Node's
`gzipSync` at the default level; source maps are excluded because they are not the
payload.

## §12's rows, measured

The spec's §12 lists the numbers the build owes back. Every row below carries its
number and the machine that produced it; the one row that cannot have a number yet
is the deployed commit's gate results, because nothing is deployed (the sequence is
in `docs/runbook.md`).

| §12 row | Number | Machine label | Artifact |
| --- | --- | --- | --- |
| `DEFAULT_BEAM` | **25** — named on cost; `default_beam_basis: "cost"`, `quality_axis: "no-resolution"` | AMD Ryzen 7 5700X (8C/16T), 31.9 GB, Windows 11 Pro; wasm through node 24.21.0 (V8); rustc 1.99.1 release, `lto=fat, codegen-units=1` | `data/measurements/width-curve.json` |
| Width cap (the slider maximum) | **800**, `cap_bound: "budget"`. 1,600 was measured and failed the 200 ms default-config p90 budget at **203.46 ms** native (≈215 ms extrapolated on wasm) | same | `data/measurements/width-curve.json` (`overshoot`) |
| Shipped evaluator and weight table | **E1 / `E1_PUBLISHED`** — Dellacherie–Thiery, 8 features; dedup and no-suicide unconditional, the Tail switchable | deterministic; the artifact records the box | `data/measurements/ablation-ladder.json`; weights in `engine/src/evaluator.rs` |
| Reference run rows | index **3** (live), both configurations: **1,000/1,000 wins**, wasm **4bfa56b5…** (58,852 B), commit `a773ce5`, 2026-11-03T03:37:10Z. Index 2 is void and says so | each row carries cpu, os, runtime, rustc and node | `data/dataset/*/reference.jsonl` |
| wasm raw size and whole-page gzip | **58,852 B** raw; **77,495 B** gzip over **234,046 B** raw, 5 files (source maps excluded) | same box; rustc 1.99.1 win-gnu with `strip = true`; Node 24.21.0 `gzipSync` default level, Vite 7.3.7 | gate 6; the per-file table below |
| Solve p90, default configuration | **4.536 ms** at beam 25, wasm (budget 200 ms) | Reference row index 3's machine | the `goal8-p40-cap7` reference row's `stats` |
| Solve p90, harder configuration | **6.547 ms** at beam 25, wasm (budget 1,000 ms) | same | the `goal12-p100-cap7` reference row's `stats` |
| First solve p75 | **5.824 ms** over 50 trials — compile + instantiate + `gen_game` + `solve`; network fetch and Worker spawn excluded (budget 1,000 ms) | same box; node 24.21.0 V8 | `node bench/tools/first-solve.mjs --trials 50 --json` |
| Run wall time | **11.7 s** for 2,000 games on 8 workers — **over §9.3's 5 s budget**; recorded there rather than adjusted | headless Chrome on the same box | `node web/scripts/browser-check.mjs` |
| The six gate results on the deployed commit | **not yet measured** — nothing is deployed. Locally all six pass and gate 6 is 9 checks, 0 skipped | the CI runner (`windows-2025`) | the release note, after `docs/runbook.md`'s deploy |
| A frozen trace fixture | `web/test/fixtures/trace-seed0-goal8-p40-cap7-band25.json` — 444 records over 20 Layers at band 25 | `solve_traced` on the default game | the committed fixture; regenerate with `web/scripts/gen-trace-fixture.mjs` |

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
`dist/` — all nine files (including `manifest.json`, `_headers` and the two source
maps) with identical `sha256` sums and identical hashed filenames. That is the "a
rebuild that changes nothing produces the same hashes" clause, verified on this box
with this toolchain.

Caveat, stated rather than hidden: the same has to hold on `windows-2025`, where the
deploy runs. The pin (`rust-toolchain.toml`) is committed and the workflow installs
from it, but the runner's rustc build is the image's, so exact cross-machine
reproducibility is **unverified**. It is also why the deploy ships the artifact
`gates.yml` produced instead of rebuilding: the question never has to be answered.

## Observed locally, and not observed

Observed, on this box, with no network and no credentials:

- `npx wrangler deploy --dry-run` (wrangler 4.156.1, offline) exits 0, "Read 10 files
  from the assets directory", `--dry-run: exiting now` — the asset set and the config
  key set parse, and nothing was uploaded;
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

**Not observed** — no claim in this repository should imply otherwise:

1. **The deploy.** No `wrangler deploy` ran against Cloudflare, no version exists, no
   version id, no rollout, no timing.
2. **The live headers.** Edge compression (zstd/gzip), live ETag values,
   `cf-cache-status: HIT`, and the `/` and `/index.html` `_headers` patterns at the
   edge. All of it was verified against `wrangler dev` (Miniflare), which is the same
   rule engine but not the same server.
3. **`/index.html`'s redirect at the edge.** Locally it answers 307 to `/`; the edge
   is documented to do the same and is unproven here.
4. **The live smoke test.** `verify.sh` and `browser-check.mjs` have never been
   pointed at `https://lab.samibentaleb.com`, because nothing is deployed there for
   the Lab.
5. **The preview URL.** `wrangler versions upload --preview-alias` has never run, so
   no preview URL exists and the dispatch path is unexercised.
6. **Rollback.** Never exercised. The command, the 100-version window and the
   `<version-id>` argument come from `wrangler rollback --help` and the docs.
7. **The GitHub Actions runs.** Neither `gates.yml` nor `deploy.yml` has run on a
   runner: no run id, no artifact, no `windows-2025` timing. The workflow YAML is
   asserted by `web/test/deploy.test.mjs` (triggers, secrets, no rollback, the smoke
   invocation), which is a static check.
8. **Token sufficiency.** The token has never been created. `Workers Scripts:Edit`
   is the documented permission group for uploading a script version; whether it can
   also attach a *new* custom domain on first deploy is **unverified** — see
   README.md's note about attaching `lab.samibentaleb.com` once out of band.
9. **The custom domain.** `lab.samibentaleb.com` currently fronts the ticket 09 probe
   Worker. Re-pointing it at `tetris-lab`, and whether the zone needs a DNS record
   created, is unrun.
10. **The Worker's request count under Workers Free.** Static-asset requests are
    documented as free and unmetered, so the 100 k/day cap should be inert; no live
    request was counted.
