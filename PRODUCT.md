# PRODUCT.md

Context for design work on the Lab. `GLOSSARY.md` is the vocabulary; this file is who
the page is for and what it has to do.

## What it is

A browser lab that answers one question: given a 20×10 board and a known sequence of
Tetris pieces, can the search clear a target number of lines, and how? One Rust engine
compiled to wasm runs inside the visitor's tab. No server compute, no account, no
database.

Live at https://lab.samibentaleb.com, served as static assets by the Cloudflare Worker
`tetris-lab`.

## Who it is for

Someone who wants to see a beam search work, and who will judge it by whether the page
tells the truth about what it measured. They arrive from the repository, not from a
search engine, and they are willing to read an explanation of the algorithm once they
trust the numbers.

## The three things a visitor does

- **Solve one game.** Set seed, goal, piece count and starting height in the Playground,
  watch the replay, read the score, the band and the per-layer table.
- **Run many games.** The Run panel solves a range of seeds at the Playground's settings
  and reports a win rate with timings, measured in this tab.
- **Check the numbers.** The benchmark solves the committed 2,000-game Dataset beside
  the reference results from the commit machine, so a visitor can compare their machine
  with a recorded one.

## Standing constraints

- A number on screen comes from the engine, the trace, the replay, the URL, a committed
  artifact, or the page's own clock, and it says which. A placeholder in words beats a
  guess.
- The page opens on the committed default game, so the first frame is a real solve.
- The engine's own words: see `GLOSSARY.md` and `web/scripts/words.mjs` for the
  vocabulary a visitor may and may not read.

## Where the rest lives

`README.md` for the repository map, `docs/adr/` for the three decisions, `docs/design/`
for the prototypes the shell was judged against, `docs/runbook.md` for deploys.
