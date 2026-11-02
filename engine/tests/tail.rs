//! Seam 1, the Tail search: §5.3's exact search over the final pieces, and the
//! three switches §9.4's Ablation turns off one at a time.
//!
//! Two things are asserted here that a wider suite cannot: a named Seed whose
//! beam-only Solve loses and whose Tail Solve wins (the demonstration is a test,
//! not a benchmark result), and that switching the Tail on never changes a result
//! the beam already reaches.

use engine::evaluator::shipped;
use engine::generator::generate_game;
use engine::search::{solve, Feature, SolveConfig, Status, Toggles};

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
        assert!(
            !tail.tail_truncated,
            "seed {seed} at goal {goal}, width {width}: the Tail's frontier cap bound,\n\
             so this game was searched approximately. Either the cap is too small\n\
             or the committed set needs a narrower width."
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

// ── one place decides the three search decisions ───────────────────────

/// Ticket 04: "Dedup, the No-suicide rule and the Tail search are each
/// independently switchable from one place, so an Ablation is a configuration
/// change and not a second engine build."
///
/// The shipped row is constructed in exactly one place, and taking one feature
/// off it leaves the other two alone: a sweep of §9.4's ladder is
/// `Toggles::SHIPPED.without(feature)` for `feature` in `FEATURES`.
#[test]
fn the_three_search_decisions_are_switchable_from_one_place() {
    assert_eq!(
        Toggles::SHIPPED,
        Toggles {
            dedup: true,
            no_suicide: true,
            tail: true,
        },
        "the shipped row has every decision on"
    );
    assert_eq!(
        SolveConfig::new(8, 25).toggles,
        Toggles::SHIPPED,
        "a SolveConfig is shipped behaviour unless it says otherwise"
    );

    // One feature off, two untouched -- each of them, on its own.
    let no_dedup = Toggles::SHIPPED.without(Feature::Dedup);
    assert_eq!((no_dedup.dedup, no_dedup.no_suicide, no_dedup.tail), (false, true, true));
    let no_suicide = Toggles::SHIPPED.without(Feature::NoSuicide);
    assert_eq!((no_suicide.dedup, no_suicide.no_suicide, no_suicide.tail), (true, false, true));
    let no_tail = Toggles::SHIPPED.without(Feature::Tail);
    assert_eq!((no_tail.dedup, no_tail.no_suicide, no_tail.tail), (true, true, false));

    // All off is reachable from the same place, and is the ladder's worst row.
    let mut all_off = Toggles::SHIPPED;
    for feature in [Feature::Dedup, Feature::NoSuicide, Feature::Tail] {
        all_off = all_off.without(feature);
    }
    assert_eq!((all_off.dedup, all_off.no_suicide, all_off.tail), (false, false, false));
}

/// Each switch acts on the search, not just on the config: the four rows §9.4
/// compares are four `SolveConfig`s over the same game, and all of them are
/// Solves, not errors.
#[test]
fn every_ablation_row_still_solves() {
    // Seed 42 is won by the beam alone, so its outcome cannot depend on the Tail;
    // seed 453 needs the Tail, which is what makes the Tail row visible.
    let won_by_beam = generate_game(42, 40, 7).expect("seed 42 in domain");
    let needs_tail = generate_game(453, 40, 7).expect("seed 453 in domain");

    let mut rows: Vec<(&str, SolveConfig)> = vec![("shipped", SolveConfig::new(8, 25))];
    for feature in [Feature::Dedup, Feature::NoSuicide, Feature::Tail] {
        let name = match feature {
            Feature::Dedup => "dedup off",
            Feature::NoSuicide => "no-suicide off",
            Feature::Tail => "tail off",
        };
        let mut cfg = SolveConfig::new(8, 25);
        cfg.toggles = Toggles::SHIPPED.without(feature);
        rows.push((name, cfg));
    }

    for (name, cfg) in &rows {
        let result = solve(
            &won_by_beam.board,
            won_by_beam.sequence.as_slice(),
            cfg,
            shipped(),
        );
        assert_eq!(result.status, Status::Ok, "row {name} must solve, not fail");
        assert!(result.won, "row {name}: seed 42 is won by the beam alone");
        assert_eq!(result.lines, 8, "row {name}: same Lines cleared");
    }

    // The Tail is the one switch that decides this Seed: on wins, off loses.
    let on = solve(
        &needs_tail.board,
        needs_tail.sequence.as_slice(),
        &rows[0].1,
        shipped(),
    );
    let off_row = rows
        .iter()
        .find(|(name, _)| *name == "tail off")
        .expect("the tail-off row");
    let off = solve(
        &needs_tail.board,
        needs_tail.sequence.as_slice(),
        &off_row.1,
        shipped(),
    );
    assert!(on.won, "seed 453 with the Tail on");
    assert!(!off.won, "seed 453 with the Tail off");
}
