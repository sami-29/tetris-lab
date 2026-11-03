# Tetris Solver

A Tetris line-clearing solver built with Flask and an adaptive beam search algorithm. Given a starting board and a fixed sequence of pieces, it finds the exact moves needed to clear a target number of lines — and visualises them step by step.

---

## Deploy (GitHub Actions → Cloudflare Workers)

The Lab ships as **https://lab.samibentaleb.com**: a static page on Cloudflare
Workers Static Assets, published by GitHub Actions. Nothing is built on
Cloudflare's side and nothing is deployed from a laptop.

```
.github/workflows/gates.yml     every push and pull request  → the six Gates
.github/workflows/deploy.yml    a green gates run on `main`  → one Worker version
```

`deploy.yml` triggers on `workflow_run` (a separate workflow file cannot use
`needs:`), filtered to a successful push run on `main`, and uploads the exact
`web/dist` tree that `gates.yml` measured — there is no second build. A
hand-triggered run (`workflow_dispatch`, inputs `ref` and `alias`) uploads a
*preview version* with `wrangler versions upload --preview-alias`, which changes no
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
(or a logged-in `wrangler deploy`) — a token limited to Workers Scripts has not been
proven sufficient for that, and pretending otherwise is how a first deploy fails at
02:00.

### After a deploy

The workflow runs the post-deploy smoke test against the live URL and fails loudly
on any mismatch. It **never rolls back** — a live-but-wrong deploy is the operator's
call, and a rollback to a known-bad version is worse than a red badge. The version
id is in the run log (`wrangler deployments list`), so reverting is one command:

```bash
cd web
npx wrangler deployments list          # the numbered versions, newest first
npx wrangler rollback <version-id> -m "why, in one line"
```

`_headers` is part of a version's payload, so a rollback restores the headers with
the assets — the page cannot end up pointing at a hashed file that is not there.

To run the same two checks by hand against any URL (including a preview):

```bash
cd web
npm run build                          # wasm + page + dist/manifest.json
bash scripts/verify.sh --url https://lab.samibentaleb.com
node scripts/browser-check.mjs --url https://lab.samibentaleb.com/
```

What is verified locally before any of this runs is in
[`docs/measurements/delivery.md`](docs/measurements/delivery.md); what was **not**
observed is listed there too.

---

## Hosting It Live for Free

The section below describes the Flask app this repository used to serve and is
superseded by the deploy above; it is deleted at cutover.

The best free option is **[Render](https://render.com)**. It supports Python/Flask natively, requires zero configuration files beyond what is already in this repo, keeps your app alive on free tier (with some cold-start delay), and has a straightforward deploy flow from GitHub.

### Step 1 — Push to GitHub

If you have not already, create a repository on GitHub and push this project to it.

```bash
git init
git add .
git commit -m "initial commit"
git remote add origin https://github.com/YOUR_USERNAME/YOUR_REPO.git
git push -u origin main
```

Make sure `.gitignore` includes at minimum:

```
__pycache__/
*.pyc
tetris_solver.db
winnable_games.csv
log.txt
```

### Step 2 — Add a `Procfile`

Render needs to know how to start the app. Create a file called `Procfile` (no extension) in the root of the repo:

```
web: gunicorn app:app
```

`gunicorn` is already in `requirements.txt`. Do not use `python app.py` in production — Flask's built-in server is not designed to handle real traffic.

### Step 3 — Deploy on Render

1. Go to [render.com](https://render.com) and sign up with your GitHub account.
2. Click **New → Web Service**.
3. Select your repository from the list.
4. Render will auto-detect Python. Fill in the fields:

| Field | Value |
|---|---|
| **Name** | anything you like |
| **Region** | closest to you |
| **Branch** | `main` |
| **Runtime** | `Python 3` |
| **Build Command** | `pip install -r requirements.txt` |
| **Start Command** | `gunicorn app:app` |
| **Instance Type** | `Free` |

5. Click **Deploy Web Service**.
6. Wait ~2 minutes for the first build. Your app will be live at `https://YOUR-APP-NAME.onrender.com`.

### Important: The Simulation tab on free tier

The simulation runs games in parallel using `multiprocessing.Pool`. On Render's free tier (shared CPU, 512 MB RAM) this will work for small runs (≤ 100 games) but may be slow or time out for large ones (1000 games). Single-game solving works fine.

If you hit timeouts, add this environment variable in the Render dashboard under **Environment**:

| Key | Value |
|---|---|
| `WEB_CONCURRENCY` | `1` |

This tells gunicorn to use a single worker, which reduces memory pressure and plays more nicely with multiprocessing on constrained hardware.

---

## Running Locally

```bash
pip install -r requirements.txt
python app.py
```

Then open [http://localhost:5000](http://localhost:5000).

To enable debug mode (auto-reload on code changes):

```bash
FLASK_DEBUG=true python app.py
```

---

## Project Structure

```
tetris-algo/
├── app.py                  Flask routes and server entry point
├── TetrisSolver.py         Adaptive beam search solver
├── TetrisGameGenerator.py  Board and sequence generation
├── tetris_engine.py        Pure Tetris game logic (drop, place, clear, score)
├── tetromino_data.py       Piece shapes pre-compiled as numpy arrays
├── requirements.txt        Python dependencies
├── Procfile                Render/gunicorn start command
├── static/
│   ├── css/styles.css      All styles
│   └── js/
│       ├── main.js         UI logic (tabs, controls, HTMX handlers)
│       └── tetris_visualization.js  D3 board renderer and animation
└── templates/
    ├── index.html          Main page
    └── how_it_works.html   Algorithm explanation
```

---

## Algorithm Overview

The solver uses **adaptive beam search** with a progressive width schedule `(1 → 5 → 20 → 100)`. It tries the narrowest beam first (fast, greedy) and only widens the search if the solution is not found. Most games are solved at width 1 or 5, making the average cost ~620 board evaluations per game versus ~70,000 for a fixed wide beam.

Board states are scored on four factors: lines cleared (dominant), aggregate height, holes, and bumpiness. See the [How It Works](/how-it-works) page for a full explanation.

---

## Alternative Free Hosting Options

| Platform | Notes |
|---|---|
| **Render** | Recommended. Native Python, easy GitHub deploy, free tier keeps app alive. |
| **Railway** | Similar to Render, slightly more generous free tier, also easy. Sign up at [railway.app](https://railway.app). |
| **Fly.io** | More powerful free tier, but requires installing the `flyctl` CLI and a config file. Better for serious projects. |
| **PythonAnywhere** | Free tier for simple Flask apps, but multiprocessing is restricted — the simulation tab will not work on the free plan. |
| **Vercel / Netlify** | Built for JavaScript frontends. Python is supported via serverless functions but multiprocessing will not work. Not recommended for this project. |
