# 14 — Is a full pannable search tree legible at width 200?

Measured on a real trace: layered beam, width 200, Dataset game `seed 0`
(goal 8, 40 pieces), 19 layers, **3,435 surviving states**. Artifact:
`docs/design/t14-tree/index.html`. The trace is a frozen fixture -- the generator (a
throwaway numpy script that depended on the Python solver) was deleted at cutover; its
recovery command is in `README.md`.

## The answer

**No — and at width 200 it is barely a tree.** A visitor cannot read 200 nodes per
layer as nodes at any zoom. They *can* read the same data as a band, with the
winning lineage drawn through it. The honest v1 shape is a **band by default, a
bounded tree on demand (top ~24 per layer), and the winning lineage always drawn**.

Three measurements carry that, and none of them could be seen from the mockup.

### 1. The full tree is a funnel, not a tree

| fact | number |
| --- | --- |
| surviving states | 3,435 across 19 layers |
| parent edges | 3,434 (every state has exactly one parent) |
| mean children per state | 1.0 |
| states with any child, per layer | 69–134 **of 200** |
| mean distinct parents per layer | 1, 34, 70, 60, 90, 115, 134, 123, 69, 89, 116, 93, 118, 119, 98, 98, 107, 107 |
| ancestors at layer 9 feeding all 200 states of layer 18 | **6**, the largest accounting for 83 |

The beam keeps 200 states out of ~3,400–6,800 candidates each layer, so *nothing
branches*: on average each survivor contributes exactly one child, half of them
contribute none, and the whole final layer descends from six states a third of the
way in. Branching is real only in the first three layers (1 → 34 → 200). Calling
this a "search tree" on screen would be the single most misleading thing in the UI:
there is nothing to explore laterally, and half the dots are dead ends.

### 2. The winning lineage spends the middle of the game near the bottom of the beam

Rank of the lineage that ends as the best final state, per layer:

```text
L1 3/34  L2 3  L3 2  L4 175  L5 148  L6 53  L7 26  L8 7  L9 18
L10 20  L11 5  L12 36  L13 31  L14 137  L15 45  L16 4  L17 25  L18 1
```

Two consequences:

- **A view that highlights "the best" path lies.** The board that looks good at
  piece 4 is the 175th; the one that wins was 137th at piece 14. Any v1 affordance
  built on "follow the leader" would walk the visitor away from the answer.
- **A bounded tree cannot contain its own output.** At `keep top 24`, the winning
  lineage disappears at layers 4–5 and 14. So the tree has to draw the winning
  lineage *outside* the bound (a pinned trail, or a node pulled to the column edge
  with a leader line). This is a requirement, not a nicety.

Greedy descent from the layer-best state also dead-ends at layer 9: the local leader
has no child. A "follow the top node" interaction would stall in the middle of the
game.

### 3. Nothing can be collapsed by shape, and dedup earns nothing

| fact | number |
| --- | --- |
| distinct column-height profiles per layer | 165–200 of 200 (199–200 late) |
| mean cells differing between two random boards of a layer | 7–19 of 200 |
| mean cells differing from that layer's best board | 6–19 |
| score spread within a layer | 12–50 points |
| median score shift per rank | 0.06–0.25 |
| candidates removed by per-layer dedup | **159 of 75,613 (0.21%)**, max 2.9% in one layer |

Boards really are different (8–9% of cells), so no shape-based collapse exists. But
the *score* differences are tiny: 200 states inside a 12–19 point band, ~0.1 point
per rank. The y-axis of a score-ordered tree is therefore a 200-node-wide seam, and
rank-based thinning is nearly arbitrary — which is exactly why the leftover
candidates look like noise rather than decisions.

Dedup is close to a no-op at width 200. Keep it in the engine (cheap, deterministic,
a correctness net at wide beams), but it is not a speed argument, and 04's "top
optimisation" framing for it does not survive contact with the trace.

## What a visitor can actually take from it

Read off the artifact at 1600×1000, screenshots in `shots/`:

| what is drawn | nodes | verdict |
| --- | --- | --- |
| full tree, width 200 | 3,435 | unreadable as nodes: at fit zoom it is a braid; zoomed in, the edges of the neighbouring 199 states dominate whatever node you are looking at |
| keep top 60 | 1,055 | busy; the crossing haze competes with the structure |
| keep top 40 | 715 | marginal — the structure is there, the haze is starting to win |
| **keep top 20–24** | 361–430 | **legible**: the funnel reads, a single lineage can be followed by eye, and die-offs are visible |
| score band, all 200 | — | legible at full width, no interaction needed |
| board cloud | — | legible but says little: the skyline barely moves layer to layer |

What has to be collapsed or aggregated to get there:

1. **State identity below the bound** → score envelope per layer (the band), which
   keeps all 200 honestly without claiming each one is explorable.
2. **Childless states** → a terminal tick instead of a node. They are half of every
   layer from layer 2 on, and drawing them as nodes is where the noise comes from.
3. **Ordering** → by score. Parent-ordering (`by parent`) makes it worse, not better:
   edges become long diagonals across the whole column, a curtain. Measured, not
   guessed — the toggle is in the artifact.
4. **Shape-based collapsing is not available** (see above). Board-level merging
   would be a lie; per-layer dedup already removed the only duplicates.

## What the interaction has to be

Everything below was needed to make 200 states inspectable at all; strip any one and
the view stops answering questions:

- **Band behind, tree in front.** The band carries the 200 you are not drawing; the
  tree carries the ~24 you are. Without the band the bound looks like the whole
  search; without the tree there is nothing to click.
- **Wheel-zoom anchored at the cursor, drag to pan, fit key.** Ordinary, and
  necessary: nothing is readable at one zoom level.
- **Click to pin a state.** The board is the payload — a tree of anonymous dots
  teaches nothing. Pinning shows the board, the score, the rank, the move, and the
  lineage that produced it.
- **`dim everything off the selection`.** With 200 per layer, dimming the rest is the
  difference between "one path through a haze" and "a haze". Required, not optional.
- **The winning lineage pinned and labelled**, drawn even when it is outside the
  bound (see §2), in a colour nothing else uses.
- **A layer ruler with the piece letter**, and per-layer numbers in words: states
  kept, candidates, dedup, score band, board distance.

## What this changes in the deliverable

08's line was "a full pannable search tree (the accepted risk — ticket 14 tests
legibility at width 200)". The test fails at "full", so the spec should carry:

1. **Default view is the band plus the winning lineage.** One picture, readable at
   200 and at 1,600-width beams, no interaction required to get the point. Settled
   with the operator on the artifact.
2. **The tree is bounded: top ~24 per layer**, sorted by score, childless states as
   ticks, dim-off-selection on click, boards on click.
3. **The winning lineage is always drawn**, visibly outside the bound where it falls
   out of it.
4. **Watch the name.** "Search tree" describes layers 0–3. Settled: the band is the
   beam (or funnel) and "tree" is reserved for the bounded drill-down; the wording
   lands in `GLOSSARY.md` during the spec pass.
5. **Hand to 15 (validation plan):** the display needs a frozen trace fixture (this
   one will do) so tree rendering can be tested without running the solver, and the
   winning lineage's rank path is the assertion that a bound of 24 still shows it.
6. **Hand to 12:** an expert beam width is affordable (13 capped it near 1,600), but
   the display does not get more legible with width — the band does, the tree does
   not. The width slider and the tree bound are separate controls.

## Caveats

- One game, one width. Seed 0 is a 19-layer game; a 100-piece game at goal 12 has five
  times the layers, and the funnel only tightens. Re-running on the regenerated
  Dataset (ticket 11's two configurations) is worth doing once the generator exists
  (ticket 17) — the shape will not change, the numbers will.
- The trace is not the shipped engine: exact tail and no-suicide pruning are absent,
  and dedup keeps the highest score rather than the state with more lines. Both would
  trim a few percent of states per layer; neither turns a funnel into a tree.
