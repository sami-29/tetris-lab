//! Evaluators: a function from a Board to an integer score (§5.4).
//!
//! Scoring is fixed point and nothing here is floating point: features are
//! integer counts, weights are `i32` in Q16 (value = `i32 / 65536`), and the sum
//! accumulates in `i64`. That is what makes the native bench and the browser
//! agree exactly, which the ablation depends on.
//!
//! The shipped default is chosen by the ablation ladder (§9.4), which is ticket
//! 04's; this crate ships the ablation control so a Solve has a score today and
//! the ladder has something to beat.

use crate::board::{Board, W};

/// Weights are `i32` at this scale: 65536 is 1.0.
pub const Q16: i32 = 65_536;

/// What a placement did, beyond the Board it produced.
///
/// The Board alone cannot express §5.4's landing height or eroded cells, so the
/// context travels with it rather than being reconstructed by each evaluator.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PlacementCtx {
    /// Lines this placement cleared, not the cumulative count.
    pub lines_cleared: u32,
    /// Row the placed piece's top cell rests at, counted from the top; -1 when
    /// the caller has no placement (the start position).
    pub landing_height: i32,
    /// Cells removed by this placement's line clears.
    pub eroded_cells: u32,
}

impl PlacementCtx {
    /// The context of a position that was not produced by a single placement.
    pub const NONE: PlacementCtx = PlacementCtx {
        lines_cleared: 0,
        landing_height: -1,
        eroded_cells: 0,
    };
}

/// A swappable scoring function. Both implementations ship behind this trait and
/// the ladder compares them on identical games (§5.4).
pub trait Evaluator {
    /// Higher is better.
    fn score(&self, board: &Board, ctx: &PlacementCtx) -> i64;

    /// The name the page and the Reference run label rows with.
    fn name(&self) -> &'static str;
}

/// E0: column heights, holes, bumpiness, and the lines term.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HeuristicWeights {
    pub height: i32,
    pub holes: i32,
    pub bumpiness: i32,
    pub lines: i32,
}

/// The ablation control (§9.4's E0).
///
/// The lines term is **live** here. In the Python solver `evaluate()` ran after
/// `clear_lines()`, so `_W_LINES` was always zero; wired correctly, a placement
/// is credited for what it cleared.
pub struct Heuristic(pub HeuristicWeights);

impl Evaluator for Heuristic {
    fn score(&self, board: &Board, ctx: &PlacementCtx) -> i64 {
        let mut sum_heights: i64 = 0;
        let mut holes: i64 = 0;
        let mut bumpiness: i64 = 0;
        let mut previous: i32 = -1;
        for column in 0..W {
            let height = board.height(column) as i32;
            sum_heights += height as i64;
            // Cells below the top of a column that are empty are holes.
            holes += (height - board.0[column].count_ones() as i32) as i64;
            if previous >= 0 {
                bumpiness += (previous - height).unsigned_abs() as i64;
            }
            previous = height;
        }
        let weights = &self.0;
        // Arithmetic shift: a negative sum floors, which is deliberate and
        // identical in every build (no float to round differently).
        ((weights.height as i64) * sum_heights
            + (weights.holes as i64) * holes
            + (weights.bumpiness as i64) * bumpiness
            + (weights.lines as i64) * ctx.lines_cleared as i64)
            >> 16
    }

    fn name(&self) -> &'static str {
        "heuristic"
    }
}

/// Weights are data, not code (§5.4), so tuning rewrites this table and nothing
/// else. Values are Q16: -51 * Q16 is -51.0.
pub const HEURISTIC_WEIGHTS: HeuristicWeights = HeuristicWeights {
    height: -51 * Q16,
    holes: -35 * Q16,
    bumpiness: -18 * Q16,
    // A cleared line is worth more than any single feature can cost, which is
    // what makes the search prefer clearing over flattening.
    lines: 100 * Q16,
};

static SHIPPED: Heuristic = Heuristic(HEURISTIC_WEIGHTS);

/// The Evaluator a Solve uses when the caller has no opinion. `&'static` so the
/// wasm crate carries no state and the choice cannot drift between calls.
pub fn shipped() -> &'static dyn Evaluator {
    &SHIPPED
}
