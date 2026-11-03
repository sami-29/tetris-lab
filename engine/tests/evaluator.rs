//! Seam: the Evaluators as arithmetic.
//!
//! §5.4's four E0 Features and eight DT Features, both committed weight tables,
//! the fixed-point rule that makes native and wasm agree, and the tests that keep
//! E0's lines term from going back to being dead code.
//!
//! The expected values in `tests/fixtures/*.tsv` are counted by hand from §5.4's
//! definitions — each row carries its board as a grid and its arithmetic as a
//! comment — and not produced by the code under test. A blessed fixture only
//! proves that the code is deterministic, which it already is.

use std::path::{Path, PathBuf};

use engine::board::{Board, W};
use engine::evaluator::{
    DellacherieThiery, DtFeatures, Evaluator, Heuristic, HeuristicWeights, PlacementCtx,
    E0_HEURISTIC, E1_PUBLISHED, Q16,
};
use engine::pieces::Piece;
use engine::search::{solve, SolveConfig};

/// §5.4's pre-registered verdict: "E1 ships unless E2 clears p < 0.01". Ticket 08
/// owns the measurement; a table here that disagrees with `shipped()` is the
/// pointer to the line the verdict rewrites.
const SHIPPED_NAME: &str = "dellacherie-thiery";

/// The compile-time half of the dead-term guard: a zeroed table is a `const`
/// error, not a slow regression.
const _: () = assert!(E0_HEURISTIC.lines != 0);

// ── fixtures ───────────────────────────────────────────────────────────────

fn fixture_rows(name: &str) -> Vec<String> {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is not readable: {error}", path.display()));
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_string)
        .collect()
}

fn board_from(words: &[&str]) -> Board {
    assert_eq!(words.len(), W, "a fixture row carries ten column words");
    let mut board = Board::EMPTY;
    for (column, word) in words.iter().enumerate() {
        board.0[column] = u32::from_str_radix(word, 16)
            .unwrap_or_else(|error| panic!("`{word}` is not a column word: {error}"));
    }
    board
}

fn fields(row: &str) -> Vec<&str> {
    row.split_whitespace().collect()
}

// ── E0, the ablation control ────────────────────────────────────────────────

#[test]
fn e0_scores_match_the_fixtures_counted_by_hand() {
    let rows = fixture_rows("e0-scores.tsv");
    assert!(rows.len() >= 8, "coverage shrank to {} rows", rows.len());
    let evaluator = Heuristic(E0_HEURISTIC);
    for row in &rows {
        let f = fields(row);
        let board = board_from(&f[..W]);
        let lines_cleared: u32 = f[W].parse().unwrap();
        let expected: i64 = f[W + 1].parse().unwrap();
        let ctx = PlacementCtx {
            lines_cleared,
            ..PlacementCtx::NONE
        };
        assert_eq!(
            evaluator.score(&board, &ctx),
            expected,
            "e0-scores.tsv row `{row}`"
        );
    }
}

/// §5.4: the Python lines term was dead — `evaluate()` ran after `clear_lines()`,
/// so the feature it multiplied was structurally zero. A `score()` that counted
/// full rows of the post-clear board would reproduce that bug exactly, and fails
/// here: none of these boards holds a full row.
#[test]
fn the_lines_term_moves_the_score_by_exactly_its_weight() {
    let board = board_from(&[
        "00000005", "00000000", "00000000", "00000000", "00000000", "00000000", "00000000",
        "00000000", "00000000", "00000000",
    ]);
    let evaluator = Heuristic(E0_HEURISTIC);
    let dead = evaluator.score(&board, &PlacementCtx::NONE);
    for cleared in 1..=4u32 {
        let ctx = PlacementCtx {
            lines_cleared: cleared,
            ..PlacementCtx::NONE
        };
        assert_eq!(
            evaluator.score(&board, &ctx) - dead,
            1_000 * cleared as i64,
            "each Line cleared is worth its full weight, not a fraction of it"
        );
    }
    // The term is exact because 1000·Q16·k is a whole multiple of Q16, so the one
    // shift at the end cannot floor it.
    assert_eq!(dead, -242, "-51*3 -35*1 -18*3 on this board");
}

#[test]
fn the_committed_e0_table_is_the_python_one() {
    // tetris_engine.py:17–20, verbatim: _W_HEIGHT = -51.0, _W_HOLES = -35.0,
    // _W_BUMPINESS = -18.0, _W_LINES = 1000.0.
    assert_eq!(E0_HEURISTIC.sum_heights, -51 * Q16);
    assert_eq!(E0_HEURISTIC.holes, -35 * Q16);
    assert_eq!(E0_HEURISTIC.bumpiness, -18 * Q16);
    assert_eq!(E0_HEURISTIC.lines, 1000 * Q16);
    assert_ne!(
        E0_HEURISTIC.lines, 0,
        "a zeroed lines weight silently restores the Python dead term"
    );
}

#[test]
fn e0_ignores_the_two_features_it_has_no_weight_for() {
    // The evidence that one context type serves both evaluators: E0 reads
    // `lines_cleared` and nothing else, and still scores.
    let board = board_from(&[
        "00000003", "00000001", "00000003", "00000000", "00000000", "00000000", "00000000",
        "00000000", "00000000", "00000000",
    ]);
    let evaluator = Heuristic(E0_HEURISTIC);
    let scored_by_lines_only = PlacementCtx {
        lines_cleared: 1,
        landing_height: 1,
        eroded_cells: 0,
    };
    let same_placement_at_the_top = PlacementCtx {
        landing_height: 20,
        eroded_cells: 16,
        ..scored_by_lines_only
    };
    assert_eq!(
        evaluator.score(&board, &scored_by_lines_only),
        evaluator.score(&board, &same_placement_at_the_top)
    );
}

// ── E1, the eight Features at published weights ─────────────────────────────

#[test]
fn e1_features_match_the_fixtures_counted_by_hand() {
    let rows = fixture_rows("e1-features.tsv");
    assert!(rows.len() >= 8, "coverage shrank to {} rows", rows.len());
    let evaluator = DellacherieThiery(E1_PUBLISHED);
    for row in &rows {
        let f = fields(row);
        let board = board_from(&f[..W]);
        // The row's eight numbers are the feature vector in wire order: landing
        // height and eroded cells first, then the six counted on the board.
        let expected: Vec<i32> = f[W..W + 8]
            .iter()
            .map(|field| field.parse().unwrap())
            .collect();
        let ctx = PlacementCtx {
            lines_cleared: 0,
            landing_height: expected[0],
            eroded_cells: expected[1] as u32,
        };
        assert_eq!(
            evaluator.features(&board, &ctx).as_array().to_vec(),
            expected,
            "e1-features.tsv row `{row}`"
        );
    }
}

#[test]
fn e1_scores_match_the_fixtures_counted_by_hand() {
    let rows = fixture_rows("e1-scores.tsv");
    assert!(rows.len() >= 8, "coverage shrank to {} rows", rows.len());
    let evaluator = DellacherieThiery(E1_PUBLISHED);
    for row in &rows {
        let f = fields(row);
        let board = board_from(&f[..W]);
        let ctx = PlacementCtx {
            lines_cleared: 0,
            landing_height: f[W].parse().unwrap(),
            eroded_cells: f[W + 1].parse().unwrap(),
        };
        let expected: i64 = f[W + 2].parse().unwrap();
        assert_eq!(
            evaluator.score(&board, &ctx),
            expected,
            "e1-scores.tsv row `{row}`"
        );
    }
}

#[test]
fn the_committed_e1_table_is_the_published_vector() {
    // Thiery & Scherrer, ICGA Journal 32 (2009), §2.1, verbatim:
    // − (Landing height) + (Eroded piece cells) − (Row transitions)
    // − (Column transitions) − 4 × (Holes) − (Cumulative wells).
    assert_eq!(E1_PUBLISHED.landing_height, -Q16);
    assert_eq!(E1_PUBLISHED.eroded_cells, Q16);
    assert_eq!(E1_PUBLISHED.row_transitions, -Q16);
    assert_eq!(E1_PUBLISHED.col_transitions, -Q16);
    assert_eq!(E1_PUBLISHED.holes, -4 * Q16);
    assert_eq!(E1_PUBLISHED.wells, -Q16);
    // Zero on purpose: the paper's §4 says hole depth and rows with holes are the
    // "two original features" BCTS added, so they are not in Dellacherie's
    // hand-tuned controller. Shipping them at zero keeps E2 a table swap.
    assert_eq!(E1_PUBLISHED.hole_depth, 0);
    assert_eq!(E1_PUBLISHED.rows_with_holes, 0);
}

#[test]
fn a_weight_table_is_the_evaluator_and_not_a_code_path() {
    // E2 is exactly this: the same type, a different value. Nothing in `score()`
    // can tell them apart, which is what "tuning rewrites data, never code" means.
    let board = board_from(&[
        "00000005", "00000000", "00000005", "00000000", "00000000", "00000000", "00000000",
        "00000000", "00000000", "00000000",
    ]);
    let ctx = PlacementCtx {
        lines_cleared: 0,
        landing_height: 2,
        eroded_cells: 0,
    };
    let published = DellacherieThiery(E1_PUBLISHED).score(&board, &ctx);
    let mut tuned = E1_PUBLISHED;
    tuned.holes = -13 * Q16; // a BCTS-shaped value, not a different evaluator
    assert_ne!(DellacherieThiery(tuned).score(&board, &ctx), published);
    assert_eq!(
        DellacherieThiery(tuned).name(),
        DellacherieThiery(E1_PUBLISHED).name(),
        "a tuned table is still Dellacherie–Thiery"
    );
}

// ── the placement the Board cannot remember ─────────────────────────────────

#[test]
fn landing_height_is_the_centre_of_mass_of_the_placed_piece() {
    let i = Piece::I.rotations();
    assert_eq!(
        i[1].landing_height(0),
        2,
        "a vertical I on the floor: (1+2+3+4)/4 = 2"
    );
    assert_eq!(
        i[0].landing_height(0),
        1,
        "a flat landing on the floor is 1, not 0 — the same convention as h(c)"
    );
    assert_eq!(
        Piece::O.rotations()[0].landing_height(0),
        1,
        "(1+1+2+2)/4 floors to 1"
    );
    assert_eq!(
        Piece::T.rotations()[0].landing_height(5),
        6,
        "(7+6+7+7)/4 floors to 6"
    );
    assert_eq!(
        i[1].landing_height(16),
        18,
        "an I standing in the top four rows has its centre at 18: (17+18+19+20)/4 floors to 18"
    );
    assert_eq!(
        i[0].landing_height(19),
        20,
        "a flat landing on the top row is 20"
    );
    // The pin that matters: the corner-footed orientations are where the centre of
    // mass and the middle of the bounding box disagree. J at rotation 3 sits on
    // cell rows 1,1,2,3, so the centre of mass is 7/4 = 1 and a bounding-box
    // centre would say 2. Picking the cheaper convention silently would be a
    // different evaluator.
    assert_eq!(Piece::J.rotations()[3].landing_height(0), 1);
}

#[test]
fn eroded_cells_counts_the_piece_cells_in_the_rows_it_cleared() {
    let i = Piece::I.rotations();
    let four_rows = 0b1111u32;
    assert_eq!(
        i[1].cells_in_rows(0, four_rows),
        4,
        "a vertical I lies in all four rows it completes: b = 4, so k*b = 16"
    );
    assert_eq!(
        i[0].cells_in_rows(0, 0b0001),
        4,
        "a horizontal I lies in the one row it completes: b = 4, so k*b = 4"
    );
    assert_eq!(Piece::O.rotations()[0].cells_in_rows(0, 0b0011), 4);
    assert_eq!(
        i[0].cells_in_rows(0, 0b1000),
        0,
        "a piece resting on a stack can complete a row it never touches: k*0 = 0"
    );
}

#[test]
fn bits_above_row_zero_cannot_inflate_a_feature() {
    // An unmasked column word makes `32 - leading_zeros()` a height above 20,
    // which inflates heights, holes, wells and both transition counts at once —
    // a parity bug that looks like a scoring bug, and would be debugged as one.
    let clean = board_from(&[
        "00000001", "00000000", "00000000", "00000000", "00000000", "00000000", "00000000",
        "00000000", "00000000", "00000000",
    ]);
    let mut dirty = clean;
    dirty.0[0] = 0x0100_0001;
    assert_eq!(dirty.height(0), 1, "a height is a count in 0..=20");
    let ctx = PlacementCtx::NONE;
    let heuristic = Heuristic(E0_HEURISTIC);
    let dt = DellacherieThiery(E1_PUBLISHED);
    assert_eq!(heuristic.score(&dirty, &ctx), heuristic.score(&clean, &ctx));
    assert_eq!(
        dt.features(&dirty, &ctx).as_array(),
        dt.features(&clean, &ctx).as_array()
    );
}

/// §5.4 and ticket 01's gotchas: a `usize` in a score expression or a `.0` float
/// literal are both invisible in review and both change the number on one target.
#[test]
fn no_float_and_no_pointer_width_arithmetic_in_the_engine() {
    let sources = std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
        .expect("the engine has sources");
    let mut checked = 0;
    for entry in sources {
        let path = entry.expect("a readable directory entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("a readable source file");
        checked += 1;
        for (index, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            for token in ["f32", "f64", "f128"] {
                assert!(
                    !code.contains(token),
                    "{}:{}: `{token}` on the scoring path — the engine is fixed point (§5.4)",
                    path.display(),
                    index + 1
                );
            }
            let chars: Vec<char> = code.chars().collect();
            for i in 1..chars.len().saturating_sub(1) {
                if chars[i] == '.' && chars[i - 1].is_ascii_digit() && chars[i + 1].is_ascii_digit()
                {
                    panic!(
                        "{}:{}: a float literal — weights are Q16 integers (§5.4)",
                        path.display(),
                        index + 1
                    );
                }
            }
        }
    }
    assert!(checked >= 6, "the walk found {checked} source files");
}

#[test]
fn the_score_bound_holds_where_the_trace_stores_an_i32() {
    // §4.4 keeps `score` as `i32`; that is safe only because the post-shift bound
    // is small. E0 is the widest of the two shipped tables at 24,440.
    const E0_MAX: [i32; 4] = [200, 200, 180, 4];
    const DT_MAX: [i32; 8] = [20, 16, 220, 210, 200, 840, 1_000, 20];
    fn bound(weights: &[i32], maxima: &[i32]) -> i64 {
        let mut total: i64 = 0;
        for (weight, maximum) in weights.iter().zip(maxima) {
            total += (*weight as i64).abs() * (*maximum as i64);
        }
        total >> 16
    }
    let e0 = bound(&E0_HEURISTIC.as_array(), &E0_MAX);
    let dt = bound(&E1_PUBLISHED.as_array(), &DT_MAX);
    assert_eq!(e0, 24_440, "-51*200 -35*200 -18*180 + 1000*4");
    assert_eq!(dt, 2_106);
    assert!(e0.max(dt) < i32::MAX as i64);

    // …and no fixture score escapes it.
    for row in fixture_rows("e1-scores.tsv")
        .iter()
        .chain(fixture_rows("e0-scores.tsv").iter())
    {
        let f = fields(row);
        if let Some(last) = f.last() {
            let score: i64 = last.parse().unwrap();
            assert!(score.abs() <= 24_440, "fixture row `{row}` scores {score}");
        }
    }
}

// ── the fixed-point rule ────────────────────────────────────────────────────

#[test]
fn the_shift_happens_once_at_the_end_and_floors() {
    // A board with Σ heights 3 and bumpiness 3, so the two half-weight terms add up
    // to exactly 3.0: per-term shifting would answer 2 (1.5→1 and 1.5→1), and `/`
    // instead of `>>` would make the negative case answer -1 where the real-valued
    // published function and wasm's `i64.shr_s` both say -2.
    let board = board_from(&[
        "00000001", "00000003", "00000000", "00000000", "00000000", "00000000", "00000000",
        "00000000", "00000000", "00000000",
    ]);
    let half = HeuristicWeights {
        sum_heights: Q16 / 2,
        holes: 0,
        bumpiness: Q16 / 2,
        lines: 0,
    };
    let ctx = PlacementCtx::NONE;
    assert_eq!(
        Heuristic(half).score(&board, &ctx),
        3,
        "1.5 + 1.5 in one sum, not 1 + 1"
    );
    let negative = Heuristic(HeuristicWeights {
        sum_heights: -(Q16 / 2),
        holes: 0,
        bumpiness: 0,
        lines: 0,
    });
    assert_eq!(negative.score(&board, &ctx), -2, "-1.5 floors to -2");
}

// ── the search builds the context ───────────────────────────────────────────

/// Records what the search hands an evaluator, so the context can be asserted
/// without the search having to expose it.
struct Probe {
    seen: std::cell::RefCell<Vec<PlacementCtx>>,
}

impl Evaluator for Probe {
    fn score(&self, board: &Board, ctx: &PlacementCtx) -> i64 {
        self.seen.borrow_mut().push(*ctx);
        Heuristic(E0_HEURISTIC).score(board, ctx)
    }

    fn name(&self) -> &'static str {
        "probe"
    }
}

#[test]
fn the_search_hands_the_evaluator_the_geometry_it_applied() {
    // Rows 0–3 are filled in columns 1–9 and column 0 is empty, so a vertical I
    // dropped into column 0 completes all four rows — and nothing else does. The
    // sequence is longer than §5.3's exact Tail on purpose: the Tail is an exact
    // search and never scores, so a one-piece game would never call the evaluator.
    let mut board = Board::EMPTY;
    for column in 1..W {
        board.0[column] = 0b1111;
    }
    let probe = Probe {
        seen: std::cell::RefCell::new(Vec::new()),
    };
    let sequence = [Piece::I, Piece::O, Piece::O, Piece::O];
    let result = solve(
        &board,
        &sequence,
        &SolveConfig::new(5, 25), // a Goal above 4, so the clearing placement is scored
        &probe,
    );
    assert_eq!(result.status, engine::search::Status::Ok);

    let seen = probe.seen.borrow();
    let four_row_clears: Vec<PlacementCtx> = seen
        .iter()
        .copied()
        .filter(|ctx| ctx.lines_cleared == 4)
        .collect();
    assert_eq!(
        four_row_clears.len(),
        1,
        "only the vertical I in column 0 clears four rows"
    );
    assert_eq!(
        four_row_clears[0],
        PlacementCtx {
            lines_cleared: 4,
            landing_height: 2, // the I's cells sit at rows 1,2,3,4
            eroded_cells: 16,  // 4 lines × 4 piece cells
        },
        "the context is the placement's own geometry, read before the clear"
    );

    for ctx in seen.iter() {
        assert!(ctx.landing_height >= 1 && ctx.landing_height <= 20);
        assert!(ctx.eroded_cells <= 16);
        if ctx.lines_cleared == 0 {
            assert_eq!(ctx.eroded_cells, 0, "no Lines, no eroded cells");
        }
    }
}

// ── one shipped evaluator, reachable one way ────────────────────────────────

#[test]
fn the_shipped_evaluator_is_the_published_one_and_reachable_one_way() {
    let shipped = engine::shipped();
    assert_eq!(
        shipped.name(),
        SHIPPED_NAME,
        "§9.4 pre-registers E1; ticket 08's verdict edits `shipped()` and this line"
    );
    // Both implementations are behind the one trait, and the ABI passes neither a
    // name nor a table: `solve` takes no evaluator argument, `SolveConfig` holds
    // only the §9.4 toggles, and there is no export that takes weights. So the
    // Playground can move the shipped table's numbers and cannot choose a model.
    assert_eq!(Heuristic(E0_HEURISTIC).name(), "heuristic");
    assert_eq!(DellacherieThiery(E1_PUBLISHED).name(), SHIPPED_NAME);
    assert_eq!(
        HeuristicWeights::FEATURES.len(),
        4,
        "E0's labels, in wire order"
    );
    assert_eq!(DtFeatures::FEATURES.len(), 8, "E1's labels, in wire order");
}
