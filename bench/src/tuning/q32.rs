//! The trainer's transport for a candidate weight vector, and the commit-time
//! conversion back to Q16.
//!
//! Cross-entropy explores continuous weights; the engine's only scoring path is
//! integer (§5.4: `i32` Q16, `i64` accumulation, no float anywhere in `engine/`).
//! The two meet here, and only here:
//!
//! * **[`TrainDt`]** is an [`engine::Evaluator`] over **Q32** `i64` weights —
//!   `Σ w·f >> 32` — so a sampled vector reaches the search at a resolution of
//!   `2^-32`, which is "real" for this purpose and costs one shift;
//! * **[`to_q16`]** rounds the winning vector to the `i32` table the engine
//!   ships, by the rule notes/05 §3.2 already states: half away from zero on the
//!   exact `decimal × 65536`.
//!
//! **Feature extraction is not duplicated.** `TrainDt` calls the engine's own
//! `DellacherieThiery::features`, so CE scores exactly the integers the committed
//! table will — a second feature implementation in `bench` would mean the trainer
//! optimised a different problem and nobody would notice.
//!
//! Q32 is a *transport format*, not a weight unit: no Q32 value is ever
//! committed. The record stores the decimal vector and the Q16 vector, and a test
//! re-derives the second from the first.

use engine::{Board, DellacherieThiery, DtWeights, Evaluator, PlacementCtx, E1_PUBLISHED};

/// The largest magnitude a sampled weight may take, in "real" units.
///
/// A runaway CE coordinate must not produce a vector that cannot be committed:
/// at `|w| ≤ 1024` the worst-case Q32 product sum is
/// `8 · 1024·2^32 · 1000 ≈ 3.5e16`, inside `i64`, and the post-shift score is
/// `≤ 8 · 1024 · 1000 = 8.19e6`, inside `i32`. It is a design choice, not a
/// published one, and it is recorded in the run's `params`.
pub const WEIGHT_BOUND: f64 = 1024.0;

/// Q32: the transport's scale.
const Q32: i64 = 1 << 32;

/// The engine's feature extractor, instantiated so `TrainDt` can borrow it. The
/// table it carries is irrelevant — `features` reads no weights.
static FEATURES: DellacherieThiery = DellacherieThiery(E1_PUBLISHED);

/// A candidate weight vector, as an [`Evaluator`] the search can rank with.
///
/// This is the *only* evaluator the trainer adds, and it is not an Evaluator in
/// §5.4's sense: it exists inside a tuning run and is never reachable from the
/// ABI, the page or `shipped()`.
pub struct TrainDt {
    /// Q32 weights, one per Feature, in [`DtFeatures::FEATURES`] order.
    w: [i64; 8],
}

impl TrainDt {
    /// A candidate, clamped to `|w| ≤ 1024` and rounded half away from zero.
    pub fn from_decimal(weights: &[f64; 8]) -> TrainDt {
        let mut w = [0i64; 8];
        for (index, value) in weights.iter().enumerate() {
            let clamped = if value.is_nan() {
                0.0
            } else {
                value.clamp(-WEIGHT_BOUND, WEIGHT_BOUND)
            };
            w[index] = round_half_away(clamped * Q32 as f64);
        }
        TrainDt { w }
    }

    /// The transport vector, for a record that wants to say what was scored.
    pub fn as_q32(&self) -> [i64; 8] {
        self.w
    }
}

impl Evaluator for TrainDt {
    #[inline]
    fn score(&self, board: &Board, ctx: &PlacementCtx) -> i64 {
        let features = FEATURES.features(board, ctx).as_array();
        let mut sum: i64 = 0;
        let mut index = 0;
        while index < 8 {
            sum += self.w[index] * features[index] as i64;
            index += 1;
        }
        // `>>` and not `/`: both Rust's `i64 >>` and wasm's `i64.shr_s` floor, the
        // same convention `engine::evaluator::fixed_point_dot` uses at Q16.
        sum >> 32
    }

    fn name(&self) -> &'static str {
        "dellacherie-thiery-training"
    }
}

/// Round half away from zero: notes/05 §3.2's rule, made executable.
///
/// `f64::round` is half away from zero too, but stating the rule here is the
/// point — a `.5` case must not be able to commit a different integer because a
/// later edit swapped in `trunc` or `round_ties_even`.
#[inline]
fn round_half_away(value: f64) -> i64 {
    if value >= 0.0 {
        (value + 0.5).floor() as i64
    } else {
        (value - 0.5).ceil() as i64
    }
}

/// The commit-time conversion: a decimal vector to the engine's Q16 table.
///
/// Half away from zero on the exact `decimal × 65536`. The input is the CE
/// output, so the rounding rule is the only thing between the run and the shipped
/// integers; `tuning_record.rs` re-derives them and fails on disagreement.
pub fn to_q16(weights: &[f64; 8]) -> [i32; 8] {
    let mut table = [0i32; 8];
    for (index, value) in weights.iter().enumerate() {
        let scaled = round_half_away(value.clamp(-WEIGHT_BOUND, WEIGHT_BOUND) * 65_536.0);
        table[index] = scaled.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    }
    table
}

/// BCTS's published cross-entropy vector, *as a baseline and never as a table we
/// ship* (notes/05 §3.2, paper §4, verbatim):
///
/// > − 12.63 (Landing height) + 6.60 (Eroded piece cells) − 9.22 (Row
/// > transitions) − 19.77 (Column transitions) − 13.08 (Holes) − 10.49
/// > (Cumulative wells) − 1.61 (Hole depth) − 24.04 (Rows with holes).
///
/// It is the one calibration we have that the trainer is in the paper's
/// neighbourhood, and it lives in `bench` on purpose: committing it beside
/// `E1_PUBLISHED` would invite someone to ship it.
pub const BCTS_DECIMAL: [f64; 8] = [-12.63, 6.60, -9.22, -19.77, -13.08, -10.49, -1.61, -24.04];

/// The same vector as the engine's own Q16 table type — so the baseline is
/// measured by the same scoring path as E1 and E2, not by a second one.
pub fn bcts_table() -> DtWeights {
    DtWeights::from_array(to_q16(&BCTS_DECIMAL))
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::Q16;

    #[test]
    fn q16_rounds_half_away_from_zero() {
        assert_eq!(
            to_q16(&[
                1.0,
                -1.0,
                0.0,
                0.5,
                -0.5,
                0.00000762939453125,
                -0.00000762939453125,
                1024.0
            ])[..5],
            [65_536, -65_536, 0, 32_768, -32_768]
        );
        // exactly .5 in Q16 units -> away from zero, both signs
        assert_eq!(to_q16(&[1.5 / 65_536.0; 8])[0], 2);
        assert_eq!(to_q16(&[-1.5 / 65_536.0; 8])[0], -2);
        assert_eq!(to_q16(&[1.4 / 65_536.0; 8])[0], 1);
        assert_eq!(to_q16(&[-1.4 / 65_536.0; 8])[0], -1);
    }

    /// notes/05 §3.2's table, which the round-half rule must reproduce exactly.
    #[test]
    fn the_bcts_table_is_the_published_one() {
        assert_eq!(
            to_q16(&BCTS_DECIMAL),
            [-827_720, 432_538, -604_242, -1_295_647, -857_211, -687_473, -105_513, -1_575_485,]
        );
    }

    /// The transport must not be a second set of features: at Q16-representable
    /// weights, `TrainDt` and `DellacherieThiery` must score identically.
    #[test]
    fn the_training_evaluator_scores_like_the_engine_one() {
        let decimal: [f64; 8] = [-12.63, 6.60, -9.22, -19.77, -13.08, -10.49, -1.61, -24.04];
        let train = TrainDt::from_decimal(&decimal);
        let engine_dt = DellacherieThiery(DtWeights::from_array(to_q16(&decimal)));

        let boards = [
            Board([0u32; 10]),
            Board([0b111 << 3, 0, 0, 0b1, 0b11 << 10, 0, 0, 0, 0, 0b1111]),
            Board([(1 << 20) - 1, 0b1010, 0, 0, 0b1_0000_0000, 0, 0, 0, 0b11, 0]),
        ];
        let contexts = [
            PlacementCtx::NONE,
            PlacementCtx {
                lines_cleared: 2,
                landing_height: 7,
                eroded_cells: 8,
            },
            PlacementCtx {
                lines_cleared: 0,
                landing_height: 19,
                eroded_cells: 0,
            },
        ];
        for board in &boards {
            for ctx in &contexts {
                let a = train.score(board, ctx);
                let b = engine_dt.score(board, ctx);
                // The two paths differ only in the scale they shift by, so at a
                // Q16-exact vector they agree exactly.
                assert_eq!(a, b, "Q32 transport disagrees with Q16 at {board:?}");
            }
        }
    }

    #[test]
    fn the_weight_bound_is_enforced() {
        let train = TrainDt::from_decimal(&[1e9, -1e9, 1024.0, -1024.0, 0.0, 0.0, 0.0, 0.0]);
        let q32 = train.as_q32();
        assert_eq!(q32[0], (WEIGHT_BOUND as i64) << 32);
        assert_eq!(q32[1], -((WEIGHT_BOUND as i64) << 32));
        assert_eq!(q32[2], 1024i64 << 32);
        assert_eq!(to_q16(&[1e9; 8])[0], (WEIGHT_BOUND as i32) * Q16);
    }
}
