# Cutover runbook: the steps only a maintainer can run

Ticket 18 is built and verified in the tree. This file is the other half: the
external steps an agent must not execute, which means no push, no rename, no
deploy, no Render command, no domain change and no merge into `main`. None of it
has been run.

Each step has a command where one exists, and a verifiable outcome. The close
condition is what the ticket asks for, not a feeling that it worked.

## 0. Preconditions

- The cutover branch is merged into `rewrite`, and `rewrite` is the tree you are
  about to deploy: `npm --prefix web run gates` green and `cargo fmt --all --check`
  clean on that commit. Record it: `git rev-parse HEAD`.
- `npm --prefix web ci` has run on this box.
- A `gates` run exists in the repository (the deploy workflow cannot name a check
  that has never completed), and the two repository secrets below exist.
- `npx wrangler login` has been done once; `npx wrangler whoami` answers.

## 1. Deploy the Lab, before moving the domain

The custom domain currently fronts the ticket-09 probe Worker, so deploying the Lab
first means there is no window where the URL answers nothing.

```bash
cd web
npm ci
npm run build
npx wrangler deploy                  # creates Worker `tetris-lab`; one numbered version
npx wrangler deployments list        # record the version id for the release note
bash scripts/verify.sh --url https://tetris-lab.<account>.workers.dev
node scripts/browser-check.mjs --url https://tetris-lab.<account>.workers.dev/
```

If the token is scoped to Workers Scripts:Edit only, attaching
`lab.samibentaleb.com` to the new Worker is a dashboard action (Workers & Pages →
`tetris-lab` → Settings → Domains & Routes). Do that here, while the probe still
exists, and only then re-run both checks against the custom domain.

**Close when:** `verify.sh` prints `PASS n checks` with 0 failed and
`browser-check.mjs` prints `PASS the page solves, plays back, and can be driven`.
That PASS is the script's own assertions: the live win rate and Evaluations equal
the Reference row's, the millisecond rows differ from it (this machine's own, and
say so), and the Run completes inside the script's 5 s budget. The script times
that budget from the click to the last game applied, which was 1.73 s on the
maintainer box on 2026-11-03, while the page's own readout for the same 2,000
games reads 12.3 s because it includes Worker start-up.
`docs/measurements/delivery.md` records both numbers and labels which is which; do
not compare the readout against the script's budget.

## 2. Delete the probe Worker

Only after the custom domain serves the Lab.

```bash
npx wrangler delete --help          # confirm the flags before the next line
npx wrangler delete --name browser-tetris-lab-probe
```

**Close when:** the probe Worker is gone from the dashboard and
https://lab.samibentaleb.com still passes both checks from step 1.

## 3. Retire Render: no redirect, no tombstone

Dashboard: Render → the service → **Settings** → **Delete Web Service** → type the
service name to confirm. Do not add a redirect, a tombstone page or a second host.

Prove the requirement rather than assuming it:

```bash
curl -sS -o /dev/null -w '%{http_code} %{redirect_url}\n' https://<service>.onrender.com
```

**Close when:** the `curl` fails with a DNS error (curl exit code 6) and prints no
HTTP status line. The name is dead, nothing answers, and there is no redirect to
record. A *suspended* service can still answer; a parked hostname would need a
support request, and the fact belongs in the release note.

## 4. Rename the repository

```bash
gh repo rename tetris-lab -R <owner>/tetris-algo
git remote set-url origin https://github.com/<owner>/tetris-lab.git
git remote -v
```

GitHub redirects web, clone, fetch and push traffic from the old name, so the old
repository URL keeps working, unlike the Render URL, which must die. Two
consequences to remember: no new repository may be created as `tetris-algo`, or the
redirect stops working; and a project-site URL is the one thing GitHub does not
redirect.

**Close when:** `git push --dry-run` succeeds through the redirect and the
repository's name in the browser is `tetris-lab`.

## 5. Merge `rewrite` into `main` as one replacement commit

`main` is an ancestor of `rewrite`, so this merge cannot conflict. `--no-ff` gives
the cutover one commit on `main` whose tree is `rewrite`'s tree.

```bash
git fetch origin
git switch main && git pull --ff-only
git merge --no-ff rewrite -m "cutover: the Lab replaces the Python app

Deletes the Flask application, its templates, static assets, requirements and
Procfile. Preserves the research notes under docs/research/ and the design
artifacts under docs/design/, reconciles the host probe's checks under
web/scripts/, rewrites README.md and .gitignore. The old code is recoverable from
git: git log -- app.py; git show 4bb9354:prototype/
"
git push origin main
```

**Close when:** the first `gates` run on `main` is green and `deploy` has run
against that commit, with the post-deploy smoke steps passing. The release note
takes its gate results from that run.

## 6. Protect `main`

After the first green `gates` run: Settings → Rules → New branch ruleset → target
`main` → Require status checks → add `gates`. GitHub only offers a check that has
completed in the repository during the past seven days, which is why this is last.

**Close when:** a pull request with a red gate cannot merge.

## 7. Write the numbers back

- **The release note:** the six gate results from the green `main` run, and the
  deployed commit's wasm raw size and whole-page gzip (gate 6 prints both, and
  `docs/measurements/delivery.md` has the local build's numbers for comparison).
- **The design spec** is the operator's local file, not part of this repository.
  Two edits are owed there: §5.7's cap sentence (every committed board is
  `<= the cap`, and `==` only for the maximum over a configuration) and §12
  rewritten as measurements. `docs/measurements/delivery.md` carries every row,
  its number and its machine label; the ticket-18 report carries the table to
  paste.
- If the deploy moved the sizes, update `docs/measurements/delivery.md` in the same
  commit as the new numbers.

## Rollback, if the deploy is wrong

The smoke test never rolls back. When a live deploy is wrong, the command is:

```bash
cd web
npx wrangler deployments list
npx wrangler rollback <version-id> -m "why, in one line"
```

`_headers` rides in the version payload, so a rollback restores the headers with
the assets.

## What was deliberately not run by an agent

No `git push`, no `gh repo rename`, no `wrangler deploy` or `delete`, no Render
command, no DNS or domain change, and no merge into `main`. The cutover was built
and verified in a branch; everything above is unexecuted.
