# web/src/ui

The panel code. Each file owns one region or one picture on the page; `main.ts` wires
them and holds the words a visitor reads.

An ordinary UI change touches these, in this order:

| File | Owns |
| --- | --- |
| `main.ts` | page wiring, every string the page writes, the paint order |
| `figure.ts` | the registry: label, unit, provenance and tooltip for each number |
| `../styles/tokens.css` | ink ramp, the six type steps, `--ctl`, the two accents |
| `../styles/shell.css` | the grid, the panels, the breakpoints |
| `playground.ts` | the four game fields and the presets |
| `run-panel.ts` | the benchmark table, its columns and machine labels |
| `../scripts/words.mjs` | the words a visitor may not read (a gate scans the build) |

Everything else is a picture or a sub-view: `band-*` and `tree-*` draw the search,
`board.ts` and `playback.ts` draw the game, `explainer.ts` holds the drawer's prose.

Two rules that catch people out:

- Copy lives in `figure.ts`, not in `index.html`. The markup carries a static copy of
  only the two figures listed in `STATIC_FIGURES`, and `test/shell.test.mjs` fails if
  the two drift or if a third appears.
- `npm run check:fast` (about 2 s) runs the typecheck and the copy-pinning tests;
  `npm run gates` is the CI bar and takes minutes.
