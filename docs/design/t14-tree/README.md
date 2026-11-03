# The ticket 14 spike: is the search tree legible at width 200?

Throwaway code from ticket 14 ("is the search tree legible at width 200?"); this
directory is the committed artifact the tree decision rests on. It exists to
answer one question: ticket 08 put a full pannable search tree in v1 at beams of
200 states per layer, and a visitor has to be able to read that. Can they, or is
it a hairball?

The answer is in [`FINDINGS.md`](FINDINGS.md). This file is how to run it.

## What is real and what is not

The data is a real beam trace: `trace.js` is a layered beam solve at width 200 of
Dataset game `seed 0` (goal 8, 40 pieces), dumped layer by layer. Every layer,
parent link, score, line count and board is what the search emitted.

It is not the shipped engine's code. It models the shape ADR-0002 describes, with a
layered beam, per-layer dedup by board, a Dellacherie–Thiery evaluator and 10×u32
column bitboards, and it leaves out the things that do not change what the tree
looks like: the exact 3-piece tail, the no-suicide height cap and worker
scheduling. The shipped engine (`engine/`) has that shape. Dedup here keeps the
highest score; the glossary says dedup keeps the state that cleared more lines.

The viewer draws no fabricated nodes and no synthetic growth curve like the t10
mockup's.

## Run it

```bash
# open the viewer: file:// is enough, trace.js is a plain script, no fetch
start docs/design/t14-tree/index.html
```

`trace.js` is a frozen fixture: it is a real width-200 beam trace of Dataset seed
0, produced by a throwaway numpy script that depended on the Python solver this
repository no longer contains
(`git show 4bb9354:prototype/t14-tree/gen_trace.py`). The viewer opens from `file://`;
there is nothing to regenerate.

The generator printed one row per layer: states kept, candidates, dedup %, score
band, mean board distance, children, ancestor concentration, and where the winning
lineage sat in that layer.

## The three views

| view | what it draws | why it exists |
| --- | --- | --- |
| **Tree** | every surviving state as a node, every placement as an edge, pannable and zoomable | the thing 08 promised, at full 200 width |
| **Score band** | one vertical band per layer, worst→best, plus the median; the winning lineage drawn through it | the same 200 states with the graph taken away |
| **Board cloud** | one averaged mini-board per layer, cell colour = share of that layer's boards filling it | where the beam actually disagrees |

Tree-only controls: order (`by score` / `by rank` / `by parent`), `keep top k`
(1-200, the collapse test), edges, the winning lineage, the greedy spine, and
`dim everything off the selection`, which is what makes width 200 readable at all.

Drive it: drag pans, wheel zooms at the cursor, click pins a node (board + lineage in
the right panel), `0` fits, `1`-`9` jumps to a layer.

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
docs/design/t14-tree/
├── index.html       the viewer: canvas tree / band / cloud, no dependencies
├── trace.js         the dumped trace + per-layer metrics (generated)
├── shots/           screenshots of each configuration, for the review
├── FINDINGS.md      the answer
└── README.md        this file
```
