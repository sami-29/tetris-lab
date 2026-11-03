# The Dataset

Two committed configurations, 2,000 seeded games each, seeds 0-1999 (test split
0-999, tuning split 1000-1999):

    goal8-p40-cap7/      goal 8,   40 pieces,  height cap 7
    goal12-p100-cap7/    goal 12, 100 pieces,  height cap 7

Each directory holds exactly three committed files:

    games.txt        one header line, then one line per game in seed order:
                     seed, ten 8-digit lowercase hex column words, sequence letters
    manifest.json    the configuration, both splits, generator_version, description,
                     and the Dataset fingerprint (FNV-1a-64 over seed + words + letters)
    reference.jsonl  the append-only Reference run, one JSON object per measurement

Bit *n* of a column word is row 19-n, so bit 0 is the bottom row. The fingerprint
is semantic: reformatting a file cannot move it, changing any game must.

`bench` is the only crate that reads or writes this directory, and it calls the
same `engine::generate_game` the wasm build ships, so "the committed games are the
games the Playground plays" is a check that runs on every push.

## Checking the Dataset

    cargo run -p bench --release -- verify

It regenerates every game from its seed, compares every board and every sequence
letter to the committed file, recomputes the fingerprint from both the file and the
regenerated games, and names the first game that moved. It writes nothing. This is
gate 1 of the six gates `.github/workflows/gates.yml` runs: a red verify blocks the
push.

`bench verify --json` prints one line per configuration for CI annotation, and

    cargo run -p bench --release -- fingerprint

prints `<config> <fingerprint> <path>` without reading the manifest.

## The cap, per game and per configuration

ADR 0003 says every committed board's tallest column is **at most** the cap, and
*equals* it only for the maximum over a configuration. Per game it is not equal: the
pre-fill loop breaks on the first draw whose resting top would exceed the cap, and
the crossing piece can overshoot by more than one row (a pre-fill line clear lowers
the stack further), so a board may be shorter than the cap. Measured over seeds
0-1999 at cap 7: 1074 boards reach exactly 7, 926 are shorter, 0 exceed it. The gate
asserts `tallest <= cap` for every game and `max over the configuration == cap`,
never per-game equality.

## When the generator legitimately changes

A generator change is never silent and never rewrites history.

1. Land the generator change, which will make `verify` name the first game that moved.
2. Re-export in place with a one-line reason:

       cargo run -p bench --release -- export --config all \
         --accept-change "pre-fill clears the completed row (bug 12)"

   The exporter refuses to write before you pass `--accept-change`. It has now
   copied the previous `games.txt` to `<config>/archive/<old-fingerprint>.txt`, written
   the new file, incremented `generator_version`, and appended that reason as one line
   in the manifest's `notes`.
3. Run a fresh Reference run and append its rows to `reference.jsonl`. Old rows stay:
   the archive file is what makes them re-derivable. Do not edit or delete an old row.
4. Commit the new `games.txt`, the new `manifest.json`, the archive file and the new
   reference rows together, with the reason in the commit message.

Rules:

- The archive file is named after the fingerprint it held and is kept forever. Never
  delete one, never overwrite one, never rename one.
- The fingerprint already is the version, so there are no versioned directories and no
  `v2/`.
- `generator_version` bumps on every change that moves a game; a change that does not
  move the fingerprint (a refactor, a comment, a faster path with identical output)
  does not bump it and does not archive anything.
- Never `git add -f`, never a `.gitignore` pattern under `data/dataset/`, never a
  hand-edited `games.txt`. If `verify` is red and you did not change the generator,
  the file is wrong.
- `git status` must be clean right after an `export` with no generator change: the
  exporter writes the same bytes. `.gitattributes` pins `data/dataset/** -text` so
  `core.autocrlf` cannot make that lie.

### Reviewing such a change

One screen, in order: `verify` green after the re-export; the archive file present,
non-empty and named after the *old* fingerprint; `generator_version` incremented
exactly once; `notes` grown by exactly one line carrying the reason; no
`reference.jsonl` row removed or edited; `git status` showing only the files above.

## Sizes

    goal8-p40-cap7/games.txt      270 949 B   (2 001 lines)
    goal12-p100-cap7/games.txt    390 951 B   (2 001 lines)

`manifest.json` is ~550 B and human-readable; `reference.jsonl` holds the append-only
Reference run, and index 3 is the live row. A generator change adds one archive file
of the same size as the `games.txt` it replaced.
