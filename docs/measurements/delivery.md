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

## §12's two rows

| Item | Value | Machine label |
| --- | --- | --- |
| wasm raw size (`engine.wasm`, the `wasm32-unknown-unknown` release artifact) | **58 302 B** (56.9 KiB) | AMD Ryzen 7 5700X 8C/16T, 31.9 GiB, Windows 11 Pro; rustc 1.99.1 (`x86_64-pc-windows-gnu`), `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = true` |
| whole-page transferred gzip (html + js + css + wasm, source maps excluded) | **68 391 B** (66.8 KiB) over 206 800 B raw, 5 files | same box; Node 24.21.0 `zlib.gzipSync` default level, Vite 7.3.7 |

Per file, so a regression is attributable:

| file | raw | gzip |
| --- | --- | --- |
| `assets/engine-BO4_EbTC.wasm` | 58 302 | 21 459 |
| `assets/index-CvTXBxIA.js` | 98 269 | 33 807 |
| `assets/index-BgWENkHr.css` | 24 617 | 5 610 |
| `index.html` | 21 408 | 5 601 |
| `assets/solve.worker-DZSqr1HT.js` | 4 204 | 1 914 |
| **total** | **206 800** | **68 391** |

Against §9.3's budgets: the wasm is 56.9 KB against a 100 KB warn line and a 150 KB
fail line; the page is 66.8 KB against a 150 KB fail line. Both pass, and gate 6
asserts them on every push.

## Reproducibility, measured

Two consecutive `npm run build` runs of the same tree produced **byte-identical**
`dist/` — all eight files (including `manifest.json` and `_headers`) with identical
`sha256` sums and identical hashed filenames. That is the "a rebuild that changes
nothing produces the same hashes" clause, verified on this box with this toolchain.

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
