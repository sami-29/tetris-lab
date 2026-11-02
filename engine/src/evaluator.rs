//! Evaluators: a function from a Board and the placement that produced it to an
//! integer score (§5.4).
//!
//! Scoring is fixed point and nothing here is floating point: features are
//! integer counts, weights are `i32` in Q16 (value = `i32 / 65536`), and the sum
//! accumulates in `i64`. That is what makes the native bench and the browser
//! agree exactly, which the ablation depends on: `i32`/`i64` add and multiply are
//! exact, so a score has one value however the optimiser schedules it.
//!
//! Two Evaluators ship behind one trait, and the difference between them is data:
//! * [`Heuristic`] — E0, the ablation control, ported from `tetris_engine.py`;
//! * [`DellacherieThiery`] — E1, the eight Features of Thiery & Scherrer at
//!   Dellacherie's published weights, and E2 once a tuning run rewrites its table.
//!
//! The shipped default is chosen by the ablation ladder (§9.4), so both
//! implementations ship and the ladder's verdict is the one line in [`shipped`].

use crate::board::{Board, FIELD, H, W};

/// Weights are `i32` at this scale: 65536 is 1.0.
pub const Q16: i32 = 65_536;

/// What a placement did, beyond the Board it produced.
///
/// The Board alone cannot express §5.4's landing height or eroded cells — a
/// post-clear Board has forgotten both — so the context travels with it. The
/// search builds it at the moment the placement is applied, which keeps every
/// evaluator a pure function of `(board, ctx)` and is why one trait serves both:
/// E0 reads one field of it and E1 three.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PlacementCtx {
    /// Lines this placement cleared, `0..=4`. Per placement, never cumulative.
    pub lines_cleared: u32,
    /// Centre of mass of the placed piece's four cells, counted from the floor,
    /// `1..=20`, taken **before** the clear. Zero means "no placement" — the
    /// start position — which is not a legal landing height, so the term
    /// contributes nothing there.
    pub landing_height: i32,
    /// `lines_cleared` × the placed piece's cells that lay in those rows,
    /// `0..=16`, counted before the clear.
    pub eroded_cells: u32,
}

impl PlacementCtx {
    /// The context of a position that no single placement produced.
    pub const NONE: PlacementCtx = PlacementCtx {
        lines_cleared: 0,
        landing_height: 0,
        eroded_cells: 0,
    };
}

/// A swappable scoring function. Both implementations ship behind this trait and
/// the ladder compares them on identical games (§5.4).
pub trait Evaluator {
    /// Higher is better. `board` is the **post-clear** board this placement
    /// produced; everything the clear destroyed is in `ctx`.
    fn score(&self, board: &Board, ctx: &PlacementCtx) -> i64;

    /// The name the page and the Reference run label rows with.
    fn name(&self) -> &'static str;
}

/// `Σ (weight × feature) >> 16`, the only place a score is produced.
///
/// Widening before multiplying and one arithmetic shift at the end are both
/// load-bearing (§5.4): `w as i32 * f as i32` overflows at Q16 scale, and a
/// per-term shift would floor each product instead of the sum, which is a
/// different (and non-portable-to-the-published-function) number. `>>` rather
/// than `/` because both Rust's `i64 >>` and wasm's `i64.shr_s` floor.
#[inline]
fn fixed_point_dot(weights: &[i32], features: &[i32]) -> i64 {
    debug_assert_eq!(weights.len(), features.len());
    let mut sum: i64 = 0;
    let mut i = 0;
    while i < weights.len() {
        sum += (weights[i] as i64) * (features[i] as i64);
        i += 1;
    }
    sum >> 16
}

// ── E0: the ported heuristic, the ablation control ──────────────────────────

/// E0's four Features, as integers. `sum_heights` is `Σ h(c)`, `holes` is
/// `Σ (h(c) − popcount(w_c))`, `bumpiness` is `Σ |h(c) − h(c+1)|` over the nine
/// adjacent pairs, and `lines` is the Lines this placement cleared.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct HeuristicFeatures {
    pub sum_heights: i32,
    pub holes: i32,
    pub bumpiness: i32,
    pub lines: i32,
}

impl HeuristicFeatures {
    /// Field order doubles as the wire order: index → feature, everywhere.
    pub const FEATURES: [&'static str; 4] = ["sum_heights", "holes", "bumpiness", "lines"];

    pub const fn as_array(&self) -> [i32; 4] {
        [self.sum_heights, self.holes, self.bumpiness, self.lines]
    }

    pub const fn from_array(features: [i32; 4]) -> HeuristicFeatures {
        HeuristicFeatures {
            sum_heights: features[0],
            holes: features[1],
            bumpiness: features[2],
            lines: features[3],
        }
    }
}

/// E0's weights, as data (§5.4).
///
/// This type is a **value, not a model selector**: it has no discriminant, no
/// name and no place in the ABI. `solve` takes no evaluator argument, the URL's
/// five keys are `seed, goal, pieces, cap, beam` and weights are not one of them
/// (§2.2), and the loader can only reach [`shipped`]. So the Playground's expert
/// controls can move these integers and cannot select a different Evaluator
/// model: whatever ships is whatever `shipped()` returns.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HeuristicWeights {
    pub sum_heights: i32,
    pub holes: i32,
    pub bumpiness: i32,
    pub lines: i32,
}

impl HeuristicWeights {
    /// Field order doubles as the wire order: index → feature, everywhere.
    pub const FEATURES: [&'static str; 4] = HeuristicFeatures::FEATURES;

    pub const fn as_array(&self) -> [i32; 4] {
        [self.sum_heights, self.holes, self.bumpiness, self.lines]
    }

    pub const fn from_array(weights: [i32; 4]) -> HeuristicWeights {
        HeuristicWeights {
            sum_heights: weights[0],
            holes: weights[1],
            bumpiness: weights[2],
            lines: weights[3],
        }
    }
}

/// E0: column heights, holes, bumpiness, and a live lines term.
///
/// The lines term is **wired**: `tetris_engine.evaluate()` ran after
/// `clear_lines()`, so `_W_LINES * lines` was multiplied by a structurally zero
/// feature at every scored placement. Here the count comes from the placement —
/// `ctx.lines_cleared` — and is credited to the post-clear board the search ranks.
pub struct Heuristic(pub HeuristicWeights);

impl Heuristic {
    /// The four Features, before the weights touch them: what the bench, the
    /// fixtures and the page's readout can quote.
    pub fn features(&self, board: &Board, ctx: &PlacementCtx) -> HeuristicFeatures {
        let mut sum_heights: i32 = 0;
        let mut holes: i32 = 0;
        let mut bumpiness: i32 = 0;
        let mut previous: i32 = -1;
        for column in 0..W {
            let height = board.height(column) as i32;
            sum_heights += height;
            // Cells below the top of a column that are empty are holes.
            holes += height - (board.0[column] & FIELD).count_ones() as i32;
            if previous >= 0 {
                bumpiness += (previous - height).unsigned_abs() as i32;
            }
            previous = height;
        }
        HeuristicFeatures {
            sum_heights,
            holes,
            bumpiness,
            lines: ctx.lines_cleared as i32,
        }
    }
}

impl Evaluator for Heuristic {
    fn score(&self, board: &Board, ctx: &PlacementCtx) -> i64 {
        // E0 ignores `landing_height` and `eroded_cells`: the Python evaluator had
        // no such Features. That one context type serves both evaluators is the
        // reason a second trait is not needed.
        fixed_point_dot(&self.0.as_array(), &self.features(board, ctx).as_array())
    }

    fn name(&self) -> &'static str {
        "heuristic"
    }
}

/// E0's weight table, as read out of `tetris_engine.py:17–20`:
/// `_W_LINES = 1000.0`, `_W_HEIGHT = -51.0`, `_W_HOLES = -35.0`,
/// `_W_BUMPINESS = -18.0`.
///
/// Committed in Q16 like every other table (§5.4 fixes one scalar for the engine),
/// and the multiplication is exact — `-51 * 65_536 = -3_342_336` — so nothing is
/// lost and the source still reads as the Python.
pub const E0_HEURISTIC: HeuristicWeights = HeuristicWeights {
    sum_heights: -51 * Q16, // −51.0 · 65_536 = -3_342_336
    holes: -35 * Q16,       // −35.0 · 65_536 = -2_293_760
    bumpiness: -18 * Q16,   // −18.0 · 65_536 = -1_179_648
    // A cleared line is worth more than the height it removes, which is what
    // makes the search prefer clearing over flattening. Live, unlike the Python.
    lines: 1000 * Q16, // 1000.0 · 65_536 = 65_536_000
};

// ── E1: Dellacherie–Thiery, eight Features ─────────────────────────────────

/// E1's eight Features, as integer counts (§5.4, Thiery & Scherrer Table 1).
///
/// The first two describe the placement and are carried in [`PlacementCtx`]; the
/// other six are counted on the post-clear board.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DtFeatures {
    /// Height of the placed piece's centre of mass above the floor, `1..=20`.
    pub landing_height: i32,
    /// `lines_cleared` × the piece's cells in the rows it removed, `0..=16`.
    pub eroded_cells: i32,
    /// Horizontal cell transitions over the twenty rows, walls filled, `0..=220`.
    pub row_transitions: i32,
    /// Vertical cell transitions over the ten columns, floor filled and sky
    /// empty, `0..=210`.
    pub col_transitions: i32,
    /// Empty cells with a filled cell somewhere above them, `0..=200`.
    pub holes: i32,
    /// `Σ depth·(depth+1)/2` over the wells, outside the board being empty,
    /// `0..=840`.
    pub wells: i32,
    /// Filled cells above each hole, summed over holes, `0..=1000`.
    pub hole_depth: i32,
    /// Rows holding at least one hole, `0..=20`.
    pub rows_with_holes: i32,
}

impl DtFeatures {
    /// Field order doubles as the wire order: index → feature, everywhere.
    pub const FEATURES: [&'static str; 8] = [
        "landing_height",
        "eroded_cells",
        "row_transitions",
        "col_transitions",
        "holes",
        "wells",
        "hole_depth",
        "rows_with_holes",
    ];

    pub const fn as_array(&self) -> [i32; 8] {
        [
            self.landing_height,
            self.eroded_cells,
            self.row_transitions,
            self.col_transitions,
            self.holes,
            self.wells,
            self.hole_depth,
            self.rows_with_holes,
        ]
    }

    pub const fn from_array(features: [i32; 8]) -> DtFeatures {
        DtFeatures {
            landing_height: features[0],
            eroded_cells: features[1],
            row_transitions: features[2],
            col_transitions: features[3],
            holes: features[4],
            wells: features[5],
            hole_depth: features[6],
            rows_with_holes: features[7],
        }
    }
}

/// E1's weights, as data (§5.4) — **the same type for E1 and E2**, because E2 is a
/// tuning run that rewrites this value and never a second Evaluator.
///
/// Like [`HeuristicWeights`], this is a value and not a model selector: no
/// discriminant, no ABI surface, no URL key, and the loader can only reach
/// [`shipped`]. The Playground's expert controls move these eight integers; they
/// cannot choose a different Evaluator model.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DtWeights {
    pub landing_height: i32,
    pub eroded_cells: i32,
    pub row_transitions: i32,
    pub col_transitions: i32,
    pub holes: i32,
    pub wells: i32,
    pub hole_depth: i32,
    pub rows_with_holes: i32,
}

impl DtWeights {
    /// Field order doubles as the wire order: index → feature, everywhere.
    pub const FEATURES: [&'static str; 8] = DtFeatures::FEATURES;

    pub const fn as_array(&self) -> [i32; 8] {
        [
            self.landing_height,
            self.eroded_cells,
            self.row_transitions,
            self.col_transitions,
            self.holes,
            self.wells,
            self.hole_depth,
            self.rows_with_holes,
        ]
    }

    pub const fn from_array(weights: [i32; 8]) -> DtWeights {
        DtWeights {
            landing_height: weights[0],
            eroded_cells: weights[1],
            row_transitions: weights[2],
            col_transitions: weights[3],
            holes: weights[4],
            wells: weights[5],
            hole_depth: weights[6],
            rows_with_holes: weights[7],
        }
    }
}

/// E1: the eight Features of Thiery & Scherrer at published weights; E2 is the
/// same type with a tuned table.
pub struct DellacherieThiery(pub DtWeights);

impl DellacherieThiery {
    /// The eight Features of `(board, ctx)`, before the weights touch them.
    pub fn features(&self, board: &Board, ctx: &PlacementCtx) -> DtFeatures {
        let mut holes = 0;
        let mut hole_depth = 0;
        let mut hole_rows: u32 = 0;
        let mut heights = [0i32; W];
        for (column, height) in heights.iter_mut().enumerate() {
            let word = board.0[column] & FIELD;
            *height = board.height(column) as i32;
            holes += *height - word.count_ones() as i32;
            // Every empty cell below the top of the column is a hole; the filled
            // cells above it are that hole's depth.
            for row in 0..*height {
                if word & (1 << row) == 0 {
                    hole_depth += (word >> (row + 1)).count_ones() as i32;
                    hole_rows |= 1 << row;
                }
            }
        }
        DtFeatures {
            landing_height: ctx.landing_height,
            eroded_cells: ctx.eroded_cells as i32,
            row_transitions: row_transitions(board),
            col_transitions: col_transitions(board),
            holes,
            wells: cumulative_wells(&heights),
            hole_depth,
            rows_with_holes: hole_rows.count_ones() as i32,
        }
    }
}

impl Evaluator for DellacherieThiery {
    fn score(&self, board: &Board, ctx: &PlacementCtx) -> i64 {
        fixed_point_dot(&self.0.as_array(), &self.features(board, ctx).as_array())
    }

    fn name(&self) -> &'static str {
        "dellacherie-thiery"
    }
}

/// Horizontal cell transitions, the left and right walls counted as filled:
/// a full row contributes nothing and an empty row contributes two.
fn row_transitions(board: &Board) -> i32 {
    let mut total = 0;
    for row in 0..H {
        let mut previous = true; // the wall left of column 0 is filled
        for column in 0..W {
            let cell = board.filled(column, row);
            if cell != previous {
                total += 1;
            }
            previous = cell;
        }
        if !previous {
            total += 1; // and the one right of column 9
        }
    }
    total
}

/// Vertical cell transitions, the cell under the floor filled and the cell above
/// row 19 empty: a solid column and an empty column both contribute one.
fn col_transitions(board: &Board) -> i32 {
    let mut total = 0;
    for column in 0..W {
        let word = board.0[column] & FIELD;
        let mut below = 1; // the cell under the floor is filled
        for row in 0..H {
            // Bit `row` is `row` cells above the floor (§5.1).
            let cell = (word >> row) & 1;
            if cell != below {
                total += 1;
            }
            below = cell;
        }
        if below != 0 {
            total += 1; // the sky above row 19 is empty
        }
    }
    total
}

/// `Σ depth·(depth+1)/2` over the wells: a column whose two neighbours are both
/// taller, its depth taken to the shorter of them. Outside the board is empty, so
/// the edge columns can never be a well.
fn cumulative_wells(heights: &[i32; W]) -> i32 {
    let mut wells = 0;
    for column in 1..W - 1 {
        let (left, own, right) = (heights[column - 1], heights[column], heights[column + 1]);
        if left > own && right > own {
            let depth = left.min(right) - own;
            wells += depth * (depth + 1) / 2;
        }
    }
    wells
}

/// E1's weight table: Dellacherie's published evaluation function, as
/// reverse-engineered by Thiery & Scherrer, *Building Controllers for Tetris*,
/// ICGA Journal 32 (2009), §2.1 — https://inria.hal.science/inria-00418954 :
///
/// > − (Landing height) + (Eroded piece cells) − (Row transitions) − (Column
/// > transitions) − 4 × (Holes) − (Cumulative wells)
///
/// All six coefficients are integers, so the Q16 conversion is exact and no
/// rounding rule is exercised. The two remaining Features are **weight zero on
/// purpose**: the paper's §4 says hole depth and rows with holes are the "two
/// original features" BCTS added, i.e. they are not in Dellacherie's hand-tuned
/// controller. Shipping eight Features with two at zero keeps E2 a pure table
/// swap (§5.4, ticket 07).
pub const E1_PUBLISHED: DtWeights = DtWeights {
    landing_height: -Q16,  // −1.0 · 65_536 = -65_536
    eroded_cells: Q16,     // +1.0 · 65_536 = 65_536
    row_transitions: -Q16, // −1.0 · 65_536 = -65_536
    col_transitions: -Q16, // −1.0 · 65_536 = -65_536
    holes: -4 * Q16,       // −4.0 · 65_536 = -262_144
    wells: -Q16,           // −1.0 · 65_536 = -65_536
    hole_depth: 0,         // absent from Dellacherie's function (paper §4)
    rows_with_holes: 0,    // as above
};

/// E2 — Dellacherie–Thiery with weights tuned by cross-entropy on the tuning
/// Split only (spec §9.4, ticket 07).
///
/// **Provenance, and do not edit these integers by hand.** They are run 0 of
/// `data/measurements/e2-tuning.json`: seed `0x9e3779b97f4a7c15`, Split
/// `1000-1999`, bed `goal12-p100-cap7`, width 25 (`DEFAULT_BEAM`, read from
/// `data/measurements/width-curve.json`), 9 iterations before the held-out
/// selection stopped improving. `bench tune --replay` re-derives this vector from
/// that seed and `bench/tests/tuning_record.rs` fails if these eight `i32` and the
/// record's `weights_q16` disagree, in either direction.
///
/// The run's verdict is `accepted` and it is the weakest kind of acceptance: every
/// table wins 1,000 of 1,000 tuning games at this width, so the bed has **no
/// win-rate resolution** (the record's `comparison.win_rate_axis` says
/// `"no-resolution"`), and E2 was not worse than E1 on the tie-breaks — mean lines
/// +0.0040 with a paired bootstrap 95 % CI of [−0.0040, +0.0130], evaluations per
/// solve 14,034 vs 14,244 at p50. §9.4's ship rule is ticket 08's and is untouched:
/// [`shipped`] still returns E1, and "E1 ships unless E2 clears p < 0.01" is a
/// comparison ticket 08 makes against this table, not one this ticket makes.
///
/// Decimal, for the record: −7.858335, 2.58857, −4.813783, −20.786974, 2.635867,
/// −6.195437, −1.74439, −11.005263 (Q16 = the integers below, round half away from
/// zero).
pub const E2_TUNED: DtWeights = DtWeights {
    landing_height: -515_004,    // −7.858335 · 65_536
    eroded_cells: 169_645,       // +2.588570 · 65_536
    row_transitions: -315_476,   // −4.813783 · 65_536
    col_transitions: -1_362_295, // −20.786974 · 65_536
    holes: 172_744,              // +2.635867 · 65_536
    wells: -406_024,             // −6.195437 · 65_536
    hole_depth: -114_320,        // −1.744390 · 65_536
    rows_with_holes: -721_241,   // −11.005263 · 65_536
};

// ── the shipped one ────────────────────────────────────────────────────────

// Both candidates stay instantiated so §9.4's verdict is a one-word edit below and
// never a code change; `strip` removes the unused one from the artifact.
#[allow(dead_code)]
static SHIPPED_HEURISTIC: Heuristic = Heuristic(E0_HEURISTIC);
static SHIPPED_DT: DellacherieThiery = DellacherieThiery(E1_PUBLISHED);

/// The Evaluator a Solve uses when the caller has no opinion.
///
/// §9.4 pre-registers the verdict: "E1 ships unless E2 clears p < 0.01 — E1 is
/// published, reproducible, and needs no training artifact in the repo", and the
/// ladder is the only thing allowed to overturn it. So this returns E1, and
/// ticket 08's verdict lands here as one line: which static, and nothing else.
///
/// `&'static` so the wasm crate carries no state and the choice cannot drift
/// between calls — and so this function is the **only** way the ABI can reach an
/// evaluator: there is no evaluator argument on `solve`, no selector in
/// `SolveConfig`, and no export that takes one, so a visitor can move the shipped
/// table's weights but cannot select a different Evaluator model (§5.4).
pub fn shipped() -> &'static dyn Evaluator {
    &SHIPPED_DT
}
