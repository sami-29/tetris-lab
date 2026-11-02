//! The Tetris Lab engine.
//!
//! Pure: no I/O, no wasm types, no JavaScript (`web/`, `engine-wasm/` and the
//! native bench all call the same code, which is what makes "the browser number
//! is not a guess" structural — ADR 0002).
//!
//! One question per entry point:
//!
//! * [`generator::generate_game`] — a Seed, a piece count and a height cap in; a
//!   Board and a Sequence out (§5.5–5.8).
//! * [`search::solve`] — a Board, its Sequence, a Goal and a Beam width in; won
//!   or lost, Lines cleared, a count of Evaluations and the Move list out (§5.3).
//! * [`search::solve_traced`] — the same result, and the survive-set of each
//!   Layer into a [`Trace`] (§4.4): §2.3's band, one record per state.
//!
//! Everything else is the vocabulary those two need.

pub mod board;
pub mod evaluator;
pub mod generator;
pub mod pieces;
pub mod search;
pub mod trace;

pub use board::Board;
pub use evaluator::{
    shipped, DellacherieThiery, DtFeatures, DtWeights, Evaluator, Heuristic, HeuristicFeatures,
    HeuristicWeights, PlacementCtx, E0_HEURISTIC, E1_PUBLISHED, Q16,
};
pub use generator::{generate_game, DomainError, Game, Sequence};
pub use pieces::{Piece, Rotation};
pub use search::{solve, solve_traced, Move, SolveConfig, SolveResult, Status};
pub use trace::{Trace, TraceRecord, PARENT_NONE, PARENT_UNRECORDED};
