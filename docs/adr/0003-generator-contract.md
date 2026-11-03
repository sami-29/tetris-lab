# Generator contract: splitmix64, purpose-separated streams, a frozen Dataset

The Dataset, the Playground and every win rate quoted on the site all rest on one function turning `(seed, pieces, height_cap)` into a board and a sequence, so its exact constants are as load-bearing as the solver's. We ship **splitmix64 with Lemire bounded draws, two streams separated by purpose (`seed ^ C_fill` and `seed ^ C_sequence`), the shipped pre-fill rule terminated before the crossing piece, and the S/Z shuffle applied at the bag seam**, with the Datasets committed as text and frozen by a semantic FNV-1a-64 fingerprint. The full contract, with every constant, is `engine/src/generator.rs`; the vectors that pin it are `engine/tests/generator_vectors.rs`, and each committed Dataset's `manifest.json` records the `generator_version` and the fingerprint. This ADR records why it has that shape.

## Considered Options

- **One stream, as the Python app had.** Rejected: the pre-fill's draw count then shifts every subsequent piece, so raising the Playground's starting height rerolls the visitor's pieces. Two streams make "same pieces, new board" true, and pair the two shipped configurations on a seed (identical board, nested sequence) instead of giving them unrelated games nobody compares.
- **Separating the streams by configuration as well as by purpose.** Rejected: 15's splits are disjoint inside a configuration, so there is no leak to close, and the pairing above is worth more than independence no query asks for.
- **xoshiro256\*\* or PCG32.** Rejected as two seeding algorithms or a `srandom` whose maths must be ported exactly, for speed this generator cannot use: it draws a few dozen numbers per game. MT19937 was out by definition — it is the RNG this effort replaces, and a CPython-compatible port was already refused in ADR 0001's spirit as permanent compatibility code.
- **`% n` for bounded draws.** Rejected for a needless 7/2⁶⁴ bias; Lemire's multiply-shift costs four lines and `u128`.
- **Placing the crossing piece so the board tops out above the cap.** Rejected: the Python rule stops before it, and the measured boards are `<= the cap` per game with `== the cap` only for the maximum over a configuration (926 of the 2,000 committed cap-7 boards are shorter), which is the assertable property the gate checks rather than a per-game equality.
- **Dropping the S/Z re-shuffle rule as dead code.** Rejected: a bag holds one of each piece, so the shipped test is unsatisfiable (zero fires in 200,000 bags), but the adjacency it exists to prevent is real and lands at bag boundaries — 200 pairs across 191 of ticket 06's 1,000 games. The rule moves to the seam rather than dying.
- **Ticket 06's ASCII-board file format.** Rejected as the committed form: ~280 bytes per game, almost all formatting. One line per game of seed, ten hex column words and the letters is the same information at half the size in the engine's own representation.
- **Committing seeds only and regenerating on demand.** Rejected: the committed file is what the fingerprint gate compares, and what keeps an old Reference row re-derivable without trusting the code of the day.
- **A SHA-256 fingerprint via a dependency.** Rejected: the value is compared for equality, never defended against an adversary, so 15 lines of FNV-1a-64 computed identically in any language beat a crate and a version bump.
- **Versioned Dataset directories.** Rejected in favour of archiving the previous file under its own fingerprint: the fingerprint already names the version, so `v2/` would be a second name for the same thing.
- **Loading `games.txt` in the browser instead of generating in wasm.** Rejected: it would honour "any seed" for 2,000 of 2³² seeds and add a second game-source path to the worker, against about a kilobyte of wasm.

## Consequences

- **The generator is a frozen interface, not a module that may drift.** Changing a constant, a draw order or a bound changes every committed board; that is gate (1) doing its job, and the only legitimate way through it is the archive-and-append procedure in the ticket.
- **Two implementations must agree, so the spec carries test vectors** (splitmix64's streams for seeds 0 and 42) and every draw order is written in prose, not implied by code. This is also what lets the generator be reviewed without running it.
- **The pre-fill clears lines**, which is invisible in the code review but changes ~0.3% of committed games at cap 7 (16 of 5,000 measured) — the reason it is written down rather than inherited.
- **The Playground's controls are bounded by the generator's domain** (seed u32, pieces 1–200, cap 2–19); a UI that offers more would be offering games the contract refuses to name.
- **`abi_version` becomes 2** when `gen_game` lands, because the loader's mismatch check is the only thing standing between a stale page and a buffer layout it does not understand.
