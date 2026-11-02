//! Seam 1, the Tail search: §5.3's exact search over the final pieces, and the
//! three switches §9.4's Ablation turns off one at a time.
//!
//! Two things are asserted here that a wider suite cannot: a named Seed whose
//! beam-only Solve loses and whose Tail Solve wins (the demonstration is a test,
//! not a benchmark result), and that switching the Tail on never changes a result
//! the beam already reaches.

use engine::evaluator::shipped;
use engine::generator::generate_game;
use engine::search::{solve, SolveConfig, Status};

mod common;
use common::{data_lines, replay};

const SEEDS: &str = include_str!("fixtures/tail-seeds.txt");

/// A configuration with the Tail switched off, which is the beam alone.
fn beam_only(goal: u32, beam_width: u32) -> SolveConfig {
    SolveConfig {
        goal,
        beam_width,
        dedup: true,
        no_suicide: true,
        tail: false,
    }
}

/// The shipped configuration: every search decision on.
fn shipped_config(goal: u32, beam_width: u32) -> SolveConfig {
    SolveConfig {
        goal,
        beam_width,
        dedup: true,
        no_suicide: true,
        tail: true,
    }
}

// ── the demonstration Seed ──────────────────────────────────────────────────

/// Seed 453 of the default Dataset configuration at beam width 25: the beam
/// alone reaches piece 40 with six of the eight Lines it needs, and the exact
/// Tail reaches eight on the last piece.
///
/// The assertion runs both directions on the same Seed, because "the Tail wins"
/// proves nothing unless the same game loses without it.
#[test]
fn the_tail_wins_a_seed_the_beam_loses() {
    let game = generate_game(453, 40, 7).expect("seed 453, 40 pieces, cap 7 is in domain");

    let beam = solve(
        &game.board,
        game.sequence.as_slice(),
        &beam_only(8, 25),
        shipped(),
    );
    assert!(!beam.won, "seed 453 must stay lost with the Tail off");
    assert_eq!(beam.lines, 6, "the beam alone clears six Lines");
    assert!(beam.moves.is_empty(), "a lost Solve returns no Move list");

    let tail = solve(
        &game.board,
        game.sequence.as_slice(),
        &shipped_config(8, 25),
        shipped(),
    );
    assert!(tail.won, "seed 453 must be won with the Tail on");
    assert_eq!(tail.lines, 8, "the Tail clears exactly the eight Lines asked");
    assert_eq!(tail.status, Status::Ok);
    assert_eq!(
        tail.moves.len(),
        game.sequence.len(),
        "the winning Move list is the whole Sequence"
    );
    // Independent replay: the Tail's Move list is a game, not a claim.
    assert_eq!(
        replay(&game.board, game.sequence.as_slice(), &tail.moves),
        8,
        "the winning Move list replays to eight Lines"
    );
    // The Tail may not cost more work than the beam it replaces: it prunes the
    // subtrees that cannot reach the Goal, so a win is cheaper than a loss here.
    assert!(
        tail.evaluations <= beam.evaluations,
        "Tail evaluations {} vs beam {}",
        tail.evaluations,
        beam.evaluations
    );

    // Determinism, in process: the same Seed twice gives the same answer.
    let again = solve(
        &game.board,
        game.sequence.as_slice(),
        &shipped_config(8, 25),
        shipped(),
    );
    assert_eq!(
        (
            again.won,
            again.lines,
            again.evaluations,
            again.moves.clone()
        ),
        (tail.won, tail.lines, tail.evaluations, tail.moves.clone()),
        "two Solves of one Seed agree exactly"
    );
}

// ── the pruning is internal, the result is not ──────────────────────────────

/// §5.3: "its internal pruning is the implementer's, its result is not".
///
/// A Goal no game can reach makes the Tail run to the end every time, which is
/// the case a naive exact Tail turns into `beam x 34 x 34 x 34` placements. The
/// admissible bound has to see that no surviving state can still reach the Goal
/// and stop before expanding it, so a hopeless Tail costs no more Evaluations
/// than the beam it replaces — not a wall-clock assertion, which would be flaky,
/// but the deterministic count that causes the wall clock.
#[test]
fn a_hopeless_tail_costs_no_more_than_the_beam() {
    let game = generate_game(0, 40, 7).expect("seed 0, 40 pieces, cap 7 is in domain");

    let beam = solve(
        &game.board,
        game.sequence.as_slice(),
        &beam_only(18, 400),
        shipped(),
    );
    let tail = solve(
        &game.board,
        game.sequence.as_slice(),
        &shipped_config(18, 400),
        shipped(),
    );

    assert!(!beam.won, "eighteen Lines is out of reach of this game");
    assert!(!tail.won, "the Tail cannot win what the board cannot hold");
    assert!(
        tail.evaluations <= beam.evaluations,
        "a hopeless Tail evaluated {} placements against the beam's {}",
        tail.evaluations,
        beam.evaluations
    );
}

// ── the committed Seed set ─────────────────────────────────────────────────

/// Ticket 04: "Tail search never changes a result the beam already reaches: for
/// the committed Seed set, a won game stays won with the same Lines cleared and
/// the same Evaluations-or-fewer."
#[test]
fn the_tail_never_changes_a_result_the_beam_reaches() {
    let mut seen = 0;
    for line in data_lines(SEEDS) {
        let f: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(f.len(), 5, "seed line: {line}");
        let goal: u32 = f[0].parse().expect("goal");
        let pieces: u32 = f[1].parse().expect("pieces");
        let cap: u32 = f[2].parse().expect("cap");
        let width: u32 = f[3].parse().expect("beam width");
        let seed: u32 = f[4].parse().expect("seed");
        seen += 1;

        let game = generate_game(seed, pieces, cap)
            .unwrap_or_else(|e| panic!("seed {seed}, {pieces} pieces, cap {cap}: {e:?}"));
        let beam = solve(
            &game.board,
            game.sequence.as_slice(),
            &beam_only(goal, width),
            shipped(),
        );
        let tail = solve(
            &game.board,
            game.sequence.as_slice(),
            &shipped_config(goal, width),
            shipped(),
        );

        assert_eq!(
            (tail.won, tail.lines),
            (beam.won, beam.lines),
            "seed {seed} at goal {goal}, width {width}: the Tail changed the result"
        );
        assert!(
            tail.evaluations <= beam.evaluations,
            "seed {seed} at goal {goal}, width {width}: {} evaluations against {}",
            tail.evaluations,
            beam.evaluations
        );
        if tail.won {
            assert_eq!(
                replay(&game.board, game.sequence.as_slice(), &tail.moves),
                tail.lines,
                "seed {seed}: the winning Move list replays to its claimed Lines"
            );
        }
    }
    assert!(seen >= 16, "the committed Seed set is not empty ({seen} seeds)");
}
