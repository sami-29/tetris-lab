//! Seam 1, the search: §5.3 as it is observable from outside — won or lost, the
//! Lines cleared, the count of Evaluations and a Move list that replays.
//!
//! The replay simulator below is deliberately written independently of the
//! engine's bitboard code: it holds the Board as a `[bool; H]` per column and
//! drops each Move with plain gravity. If the engine's move list is a fiction,
//! this is where it shows.

use engine::board::{Board, H, W};
use engine::evaluator::shipped;
use engine::generator::generate_game;
use engine::pieces::{Piece, Rotation};
use engine::search::{solve, Move, SolveConfig, Status, MAX_WIDTH};

// ── an independent replay simulator ─────────────────────────────────────────

struct Sim {
    cells: [[bool; H]; W],
}

impl Sim {
    fn from_board(board: &Board) -> Sim {
        let mut cells = [[false; H]; W];
        for (c, word) in board.0.iter().enumerate() {
            for bit in 0..H {
                cells[c][bit] = word & (1 << bit) != 0;
            }
        }
        Sim { cells }
    }

    /// Drop `rot` down column `column` under gravity; place it and clear rows.
    /// Returns the number of lines cleared, or `None` if the placement is
    /// illegal at spawn or overlaps anything.
    fn play(&mut self, piece: Piece, rotation: u8, column: u8) -> Option<u32> {
        let rots = piece.rotations();
        let rot: &Rotation = rots.get(rotation as usize)?;
        let (w, h) = (rot.width as usize, rot.height as usize);
        let col = column as usize;
        if col + w > W {
            return None;
        }
        // Cell offsets from the piece's lowest occupied bit: `Rotation.cols` is a
        // bit per row with bit 0 at the box's *bottom* row, so box row `r` is
        // offset `h - 1 - r` from that bit.
        let mut cells = Vec::new();
        for dc in 0..w {
            for row in 0..h {
                if (rot.cols[dc] >> (h - 1 - row)) & 1 == 1 {
                    cells.push((dc, h - 1 - row));
                }
            }
        }
        if cells.is_empty() {
            return None;
        }
        // Gravity: enter at the top row and fall while the position is free, so
        // the landing is the lowest base reached with every base above it free.
        // A piece that cannot enter at the top row has no legal placement.
        let mut landing: Option<usize> = None;
        for base in (0..=(H - h)).rev() {
            let fits = cells
                .iter()
                .all(|(dc, offset)| !self.cells[col + dc][base + offset]);
            if !fits {
                break;
            }
            landing = Some(base);
        }
        let base = landing?;
        for (dc, offset) in &cells {
            self.cells[col + dc][base + offset] = true;
        }

        // Clear full rows bottom-up. `bit` is a bit index, so clearing the row at
        // `bit` shifts everything above it down one place; the row that falls into
        // `bit` has to be re-checked, hence `bit += 1`.
        let mut cleared = 0u32;
        let mut bit = H;
        while bit > 0 {
            bit -= 1;
            if (0..W).all(|c| self.cells[c][bit]) {
                for c in 0..W {
                    for k in bit..(H - 1) {
                        self.cells[c][k] = self.cells[c][k + 1];
                    }
                    self.cells[c][H - 1] = false;
                }
                cleared += 1;
                bit += 1;
            }
        }
        Some(cleared)
    }

    fn as_board(&self) -> Board {
        let mut board = Board::EMPTY;
        for c in 0..W {
            for r in 0..H {
                if self.cells[c][r] {
                    board.0[c] |= 1 << r;
                }
            }
        }
        board
    }
}

fn config(goal: u32, beam_width: u32) -> SolveConfig {
    SolveConfig {
        goal,
        beam_width,
        dedup: true,
        no_suicide: true,
        tail: true,
    }
}

fn seed_42_game() -> engine::generator::Game {
    generate_game(42, 40, 7).expect("seed 42, 40 pieces, cap 7 is in domain")
}

// ── the outcome contract ────────────────────────────────────────────────────

#[test]
fn a_known_seed_solves_to_a_won_result() {
    let game = seed_42_game();
    let result = solve(
        &game.board,
        game.sequence.as_slice(),
        &config(8, 25),
        shipped(),
    );

    assert_eq!(result.status, Status::Ok);
    assert!(result.won, "seed 42 at width 25 clears 8 lines");
    assert!(result.lines >= 8, "lines {} vs goal 8", result.lines);
    assert!(result.evaluations > 0, "a solve evaluates placements");
    assert!(!result.moves.is_empty());
    assert!(result.moves.len() <= game.sequence.len());
}

#[test]
fn the_move_list_replays_to_the_claimed_lines() {
    for (seed, goal) in [(42u32, 8u32), (0, 8), (7, 8)] {
        let game = generate_game(seed, 40, 7).expect("in domain");
        let result = solve(
            &game.board,
            game.sequence.as_slice(),
            &config(goal, 25),
            shipped(),
        );
        assert!(result.won, "seed {seed} should win at width 25");

        let mut sim = Sim::from_board(&game.board);
        let mut lines = 0u32;
        for (i, mv) in result.moves.iter().enumerate() {
            let piece = game.sequence.as_slice()[i];
            assert_eq!(mv.piece_index, piece.index(), "move {i} names its piece");
            let before = sim.as_board();
            let cleared = sim.play(piece, mv.rotation, mv.column).unwrap_or_else(|| {
                panic!(
                    "seed {seed}: move {i} ({mv:?}) is not legal; board {:?}",
                    before.0
                )
            });
            lines += cleared;
        }
        assert_eq!(lines, result.lines, "seed {seed}: replayed lines");
        assert!(lines >= goal);
        assert!(sim.as_board().is_legal());
    }
}

#[test]
fn an_unreachable_goal_is_a_lost_result_with_no_moves() {
    // Five pieces cannot clear 80 lines; the search must say so plainly.
    let game = generate_game(3, 5, 7).expect("in domain");
    let result = solve(
        &game.board,
        game.sequence.as_slice(),
        &config(80, 25),
        shipped(),
    );
    assert_eq!(result.status, Status::Ok, "a loss is not an error");
    assert!(!result.won);
    assert!(result.moves.is_empty());
    assert!(result.lines < 80);
}

#[test]
fn solving_is_deterministic() {
    let game = seed_42_game();
    let a = solve(
        &game.board,
        game.sequence.as_slice(),
        &config(8, 25),
        shipped(),
    );
    let b = solve(
        &game.board,
        game.sequence.as_slice(),
        &config(8, 25),
        shipped(),
    );
    assert_eq!(a.won, b.won);
    assert_eq!(a.lines, b.lines);
    assert_eq!(a.evaluations, b.evaluations);
    assert_eq!(a.moves, b.moves);
}

// ── status codes (§4.2) ─────────────────────────────────────────────────────

#[test]
fn out_of_domain_parameters_return_status_1() {
    let game = seed_42_game();
    for cfg in [
        config(0, 25),            // goal has no zero
        config(8, 0),             // beam width starts at 1
        config(8, MAX_WIDTH + 1), // and stops at 16,384
    ] {
        let r = solve(&game.board, game.sequence.as_slice(), &cfg, shipped());
        assert_eq!(r.status, Status::OutOfDomain, "cfg {cfg:?}");
        assert_eq!(r.status.code(), 1);
        assert!(!r.won);
        assert!(r.moves.is_empty());
    }
    // The widest legal beam is inside the domain.
    assert_eq!(
        solve(
            &game.board,
            game.sequence.as_slice(),
            &config(8, MAX_WIDTH),
            shipped()
        )
        .status,
        Status::Ok
    );
}

#[test]
fn an_illegal_board_returns_status_3() {
    let game = seed_42_game();
    let mut board = game.board;
    board.0[2] = 1 << 24; // above the field
    let r = solve(&board, game.sequence.as_slice(), &config(8, 25), shipped());
    assert_eq!(r.status, Status::IllegalBoard);
    assert_eq!(r.status.code(), 3);
}

#[test]
fn status_codes_are_the_abi_numbers() {
    assert_eq!(Status::Ok.code(), 0);
    assert_eq!(Status::OutOfDomain.code(), 1);
    assert_eq!(Status::BoundExhausted.code(), 2);
    assert_eq!(Status::IllegalBoard.code(), 3);
    assert_eq!(Status::IllegalAbiState.code(), 4);
}

// ── the packed result (§4.2) ────────────────────────────────────────────────

#[test]
fn a_move_packs_piece_rotation_column() {
    let mv = Move {
        piece_index: 4,
        rotation: 3,
        column: 9,
    };
    assert_eq!(mv.packed(), (4 << 16) | (3 << 8) | 9);
    assert_eq!(Move::unpack(mv.packed()), mv);
}

#[test]
fn the_packed_result_carries_won_evals_status_lines_and_move_count() {
    let game = seed_42_game();
    let r = solve(
        &game.board,
        game.sequence.as_slice(),
        &config(8, 25),
        shipped(),
    );
    let p = r.packed();
    assert_eq!(p & 1, 1, "won is bit 0");
    assert_eq!(
        (p >> 1) & 0xffff_ffff,
        r.evaluations as i64,
        "evaluations are bits 1-32"
    );
    assert_eq!(
        (p >> 33) & 0xff,
        r.status.code() as i64,
        "status is bits 33-40"
    );
    assert_eq!((p >> 41) & 0xff, r.lines as i64, "lines are bits 41-48");
    assert_eq!(
        (p >> 49) & 0x1ff,
        r.moves.len() as i64,
        "move count is bits 49-57"
    );
    assert_eq!(p >> 58, 0, "the reserved top bits stay zero");

    // A status other than 0 must never look like a win.
    let bad = solve(
        &game.board,
        game.sequence.as_slice(),
        &config(0, 25),
        shipped(),
    );
    assert_eq!(bad.packed() & 1, 0);
}

// ── the evaluator is the only scoring surface ───────────────────────────────

#[test]
fn the_shipped_evaluator_scores_integers_and_prefers_line_clears() {
    // §5.4: credit for lines cleared is live (the Python term was dead).
    let ev = shipped();
    let board = Board::EMPTY;
    let none = ev.score(&board, &engine::evaluator::PlacementCtx::default());
    let cleared = ev.score(
        &board,
        &engine::evaluator::PlacementCtx {
            lines_cleared: 1,
            ..Default::default()
        },
    );
    assert!(cleared > none, "clearing a line must be worth something");
    assert!(!ev.name().is_empty());
}
