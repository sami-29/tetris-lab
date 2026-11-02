# host-probe — ticket 09 fixture

Throwaway proof for [ticket 09](../.scratch/browser-tetris-lab/issues/09-provision-host-and-prove-headers.md):
that the host research 02 picked actually serves this project's kind of app. It is
not the Lab, and nothing here should survive the spec (ticket 12). Delete it when
ticket 11 settles the repo story.

## What it proves

| Claim | Where it is checked |
| --- | --- |
| An assets-only Wrangler config deploys with no Worker script | `wrangler.jsonc` (no `main`), `npx wrangler deploy` |
| `.wasm` arrives as `application/wasm` to a module worker | `verify.sh` step 2, `browser-check.mjs` |
| A fingerprinted asset gets `max-age=31536000, immutable` | `src/_headers`, `verify.sh` step 2 |
| A worker can `instantiateStreaming` the module and run its exports | `src/worker.js`, `browser-check.mjs` |
| SPA fallback serves `index.html` for unknown paths | `verify.sh` step 4 |
| A custom subdomain can front the same asset set | `apply-route.sh` + `wizard.sh` stage 5 |

COOP/COEP are deliberately absent: ticket 05 settled that v1 runs one wasm
instance per worker with no `SharedArrayBuffer`. `src/_headers` keeps the
two-line opt-in commented for the day a threaded build appears.

## Layout

```
host-probe/
├── rs/                  # 212 B wasm kernel: abi_version, add, probe_bits
├── src/
│   ├── index.html.tpl   # page; __WASM_PATH__ is filled by build.sh
│   ├── worker.js        # module worker: fetch + instantiateStreaming + compute
│   └── _headers         # wasm MIME, immutable caching, no COOP/COEP
├── wrangler.jsonc       # assets-only config; ROUTE line for the subdomain
├── build.sh             # cargo -> dist/, content-hashed filename, manifest.json
├── verify.sh            # curl assertions against any base URL
├── browser-check.mjs    # headless Chrome via CDP: proves a worker runs the wasm
├── apply-route.sh       # set or clear the custom-domain route
├── wizard.sh            # operator walkthrough: login, deploy, verify, record
└── dist/                # build output (gitignored)
```

## Commands

```bash
./build.sh                                  # cargo -> dist/, fingerprinted wasm
npx wrangler dev --port 8791                # local static server, same config
./verify.sh http://127.0.0.1:8791           # header/MIME/fallback assertions
node browser-check.mjs http://127.0.0.1:8791  # headless-Chrome worker proof
./wizard.sh                                 # the human half: login + deploy
```

`wrangler` is a devDependency of this folder, so `npm install` here is enough;
`npx wrangler ...` also works without it.

## Proven live, 2026-11-02

Worker `browser-tetris-lab-probe`, zone `samibentaleb.com` (Free), served at
**https://lab.samibentaleb.com**. Wrangler 4.156.1, Chrome 142 headless, Node 24.21:

- wasm 212 B → `application/wasm`, `public, max-age=31536000, immutable`, ETag,
  `cf-cache-status: HIT` on a repeat fetch, `content-encoding: zstd`
- HTML keeps `max-age=0, must-revalidate`; unknown paths return `index.html` (200)
- module worker: `instantiateStreaming` 60–66 ms cold, then `abi_version()`=1,
  `add(2,40)`=42, `probe_bits(5e6, 7)` in 85–101 ms (~450–550M word ops/s)
- 4 files, 0.31 KiB gzipped on upload; Cloudflare created the DNS record and
  certificate for the subdomain with no manual step

Gotcha: adding a `routes` entry silently disables `workers_dev` and preview URLs,
so the `.workers.dev` name 404s until `"workers_dev": true` / `"preview_urls": true`
are set explicitly. Ticket 16 owns that choice.

## Measured locally, before the deploy

Node 24.21, Wrangler 4.156.1, Chrome headless 142, Windows 11, `wrangler dev`:

- wasm module 212 B; `instantiateStreaming` 9–14 ms in the worker
- `probe_bits(5_000_000, 7)` — 50M multiply-add steps over 20-bit column words —
  91–118 ms, so ~450–550M word ops/s in a wasm32 worker
- wasm response: `application/wasm`, `public, max-age=31536000, immutable`,
  ETag present, gzip locally (edge compression is measured live by the wizard)
- `crossOriginIsolated: false`, which is expected and allowed in v1
- local `wrangler dev` compresses with gzip; the live edge used zstd

These are a fixture's numbers on one laptop. The Lab's budget comes from ticket
06's kernel, not from this file.

## After the deploy

The live URL, the config file, the deploy command and the header evidence are in
ticket 09's `## Answer`. `host-probe/.env` keeps `PROBE_URL` / `PROBE_DOMAIN`, so
`./verify.sh` and `node browser-check.mjs` can be pointed at it with no arguments.
Neither CI nor secrets are set up here: ticket 16 owns the pipeline and any
Cloudflare API token.

`wizard.sh` stays as the repeatable path for a fresh account or machine; the host
is already provisioned, so it is optional now.
