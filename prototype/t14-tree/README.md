# Ticket 14 spike — is the search tree legible at width 200?

Throwaway code from `.scratch/browser-tetris-lab/issues/14-prototype-search-tree.md`.
It exists to answer one question: 08 put a **full pannable search tree** in v1 at
beams of 200 states per layer — can a visitor read that, or is it a hairball?

The answer is in [`FINDINGS.md`](FINDINGS.md). This file is how to run it.

## What is real and what is not

The data is a **real beam trace**: `trace.js` is a layered beam solve at width 200 of
Dataset game `seed 0` (goal 8, 40 pieces), dumped layer by layer. Every layer, parent
link, score, line count and board is what the search emitted.

It is **not** the shipped engine (that does not exist yet). It models the shape
ADR-0002 describes — layered beam, per-layer dedup by board, Dellacherie–Thiery
evaluator, 10×u32 column bitboards — and leaves out things that do not change what
the tree looks like: the exact 3-piece tail, the no-suicide height cap, worker
scheduling. Dedup here keeps the highest score; the glossary says dedup keeps the
state that cleared more lines.

The viewer itself is honest: no fabricated nodes, no synthetic growth curve like the
t10 mockup's.

## Run it

```bash
# 1. regenerate the trace (23 s, numpy; writes trace.js, ~260 KB)
python prototype/t14-tree/gen_trace.py --seed 0 --width 200 --goal 8

# 2. open the viewer — file:// is enough, trace.js is a plain script, no fetch
start prototype/t14-tree/index.html
```

`gen_trace.py` prints one row per layer: states kept, candidates, dedup %, score
band, mean board distance, children, ancestor concentration, and where the winning
lineage sat in that layer.

## The three views

| view | what it draws | why it exists |
| --- | --- | --- |
| **Tree** | every surviving state as a node, every placement as an edge, pannable and zoomable | the thing 08 promised, at full 200 width |
| **Score band** | one vertical band per layer, worst→best, plus the median; the winning lineage drawn through it | the same 200 states with the graph taken away |
| **Board cloud** | one averaged mini-board per layer, cell colour = share of that layer's boards filling it | where the beam actually disagrees |

Tree-only controls: order (`by score` / `by rank` / `by parent`), `keep top k`
(1–200, the collapse test), edges, the winning lineage, the greedy spine, and
`dim everything off the selection` — which is what makes width 200 readable at all.

Drive it: drag pans, wheel zooms at the cursor, click pins a node (board + lineage in
the right panel), `0` fits, `1`–`9` jumps to a layer.

Presets travel in the URL, so a specific state of the discussion can be linked:

```text
index.html?k=20                       # the legible bound
index.html?k=200&order=bary           # parent ordering, for comparison
index.html?view=ribbon                # the band
index.html?view=cloud                 # the averaged boards
index.html?z=7&l=4                    # zoomed into one layer
```

## Files

```text
prototype/t14-tree/
├── gen_trace.py     beam search + metrics -> trace.js   (the only real code)
├── index.html       the viewer: canvas tree / band / cloud, no dependencies
├── trace.js         the dumped trace + per-layer metrics (generated)
├── shots/           screenshots of each configuration, for the review
├── FINDINGS.md      the answer
└── README.md        this file
```
