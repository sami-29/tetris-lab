//! Seam 1, the trace: §4.4's record layout as `solve_traced` produces it.
//!
//! The invariant set here is the one `web/test/trace-fixture.test.mjs` (the
//! committed fixture, no solver) and `web/test/solve-traced.test.mjs` (the built
//! wasm) assert again — the same claims at three seams, because each seam can be
//! wrong without the others noticing:
//!
//! * this file catches an engine change on every `cargo test`;
//! * the wasm test catches the ABI path (five `i32` words, the `alloc`'d region,
//!   the count word) which no native test can see;
//! * the fixture test catches a loader or a band model that re-derives what the
//!   engine already said.
//!
//! The five load-bearing claims:
//!
//! 1. `solve_traced` **is** `solve` plus a sink: same packed `i64`, same Move
//!    list. A trace that changed the search would be a different engine.
//! 2. A layer's records are its survive-set, in layer order then rank order, and
//!    rank 0 is the best score (§4.4, §5.3): a layer's scores never increase with
//!    rank.
//! 3. Every `parent_rank` resolves in the previous layer, or is one of the two
//!    sentinels this ticket defines (`-1` layer 0 per §4.4, `-2` recorded-but-not-
//!    kept when a band narrower than the beam clipped the layer). A dangling
//!    pointer would make the lineage and the tree unbuildable.
//! 4. `band` bounds the records and *only* the records: it never changes a
//!    result, and a buffer sized from it can never be overrun.
//! 5. `lines` is cumulative along a parent edge, the free invariant that catches
//!    a per-placement/cumulative mix-up without a Board in sight.

use engine::board::Board;
use engine::evaluator::{shipped, PlacementCtx};
use engine::generator::{generate_game, Game};
use engine::search::{solve, solve_traced, SolveConfig, SolveResult, Status};
use engine::trace::{Trace, TraceRecord};

/// The shipped Beam width: `DEFAULT_BEAM` of `data/measurements/width-curve.json`,
/// which is the `band` §4.4 says the app passes (§2.3: the band *is* the beam).
/// Named once here; the node tests read it from the page's own module.
const BAND: u32 = 25;

/// notes/05 §3.4: the worst |score| any shipped weight table can produce, which is
/// why §4.4's `score: i32` is exact rather than truncated.
const SCORE_BOUND: i64 = 24_440;

/// The page's default game (`main.ts`'s `DEFAULTS`): seed 0, 40 pieces, cap 7,
/// goal 8, beam 25.
fn default_game() -> Game {
    generate_game(0, 40, 7).expect("seed 0, 40 pieces, cap 7 is in domain")
}

fn traced(game: &Game, goal: u32, beam_width: u32, band: u32) -> (SolveResult, Trace) {
    let mut trace = Trace::new();
    let result = solve_traced(
        &game.board,
        game.sequence.as_slice(),
        &SolveConfig::new(goal, beam_width),
        shipped(),
        band,
        &mut trace,
    );
    (result, trace)
}

/// The trace split by layer, in record order. The record order *is* the layer
/// order (§4.4), so this groups and never sorts.
fn layers(trace: &Trace) -> Vec<Vec<TraceRecord>> {
    let mut layers: Vec<Vec<TraceRecord>> = Vec::new();
    for record in &trace.records {
        let layer = record.layer as usize;
        if layers.len() <= layer {
            layers.resize(layer + 1, Vec::new());
        }
        layers[layer].push(*record);
    }
    layers
}

fn counts(trace: &Trace) -> Vec<usize> {
    layers(trace).iter().map(|layer| layer.len()).collect()
}

/// Claims 1, 2 and 3 of this file's header, over a whole trace.
fn assert_invariants(trace: &Trace, band: u32) {
    let layers = layers(trace);
    assert!(!layers.is_empty(), "a traced solve records at least the root");

    // Claim 2: layer order, then rank order, and rank 0 is the best score.
    for (layer_index, layer) in layers.iter().enumerate() {
        assert!(!layer.is_empty(), "layer {layer_index} exists but holds no records");
        assert!(
            layer.len() <= band as usize,
            "layer {layer_index} keeps {} records, band is {band}",
            layer.len()
        );
        for (rank, record) in layer.iter().enumerate() {
            assert_eq!(record.layer as usize, layer_index, "layer order");
            assert_eq!(record.rank as usize, rank, "rank is the index inside its layer");
            if rank > 0 {
                assert!(
                    record.score <= layer[rank - 1].score,
                    "layer {layer_index} rank {rank} scores {} above rank {}'s {}",
                    record.score,
                    rank - 1,
                    layer[rank - 1].score
                );
            }
            assert!(
                (record.score as i64).abs() <= SCORE_BOUND,
                "score {} escapes the shipped tables' bound",
                record.score
            );
        }
    }

    // Claim 3: every parent resolves in the previous layer, or is a sentinel.
    for (layer_index, layer) in layers.iter().enumerate() {
        for record in layer {
            if layer_index == 0 {
                assert_eq!(record.parent_rank, -1, "layer 0's parent is -1 by §4.4");
                continue;
            }
            let previous = &layers[layer_index - 1];
            if record.parent_rank == -2 {
                // Recorded-but-not-kept: only honest when the layer really clipped.
                assert_eq!(
                    previous.len(),
                    band as usize,
                    "a -2 parent in layer {layer_index} needs layer {} clipped at the band, it holds {} records",
                    layer_index - 1,
                    previous.len()
                );
                continue;
            }
            assert!(
                record.parent_rank >= 0 && (record.parent_rank as usize) < previous.len(),
                "layer {layer_index} rank {} names parent {}, layer {} holds {} records",
                record.rank,
                record.parent_rank,
                layer_index - 1,
                previous.len()
            );
            assert_eq!(
                previous[record.parent_rank as usize].rank as i32, record.parent_rank,
                "the parent's own record must name the same rank"
            );
        }
    }

    // Claim 5: lines are cumulative, never per placement.
    for (layer_index, layer) in layers.iter().enumerate().skip(1) {
        for record in layer {
            if record.parent_rank < 0 {
                continue;
            }
            let parent = &layers[layer_index - 1][record.parent_rank as usize];
            assert!(
                record.lines >= parent.lines,
                "layer {layer_index} rank {} has {} lines below its parent's {}",
                record.rank,
                record.lines,
                parent.lines
            );
        }
    }
}

// ── the trace is a record of the search, not a second search ────────────────

#[test]
fn a_traced_solve_returns_what_the_untraced_solve_returns() {
    for (seed, goal, beam) in [(0u32, 8u32, 25u32), (42, 8, 25), (0, 8, 1), (7, 12, 8)] {
        let game = generate_game(seed, 40, 7).expect("in domain");
        let plain = solve(
            &game.board,
            game.sequence.as_slice(),
            &SolveConfig::new(goal, beam),
            shipped(),
        );
        let (traced_result, trace) = traced(&game, goal, beam, BAND);
        assert_eq!(
            traced_result.packed(),
            plain.packed(),
            "seed {seed} goal {goal} beam {beam}: the packed result moved"
        );
        assert_eq!(
            traced_result.moves, plain.moves,
            "seed {seed} goal {goal} beam {beam}: the Move list moved"
        );
        assert!(!trace.is_empty(), "seed {seed}: recorded nothing");
    }
}

#[test]
fn the_band_bounds_the_records_and_never_the_search() {
    let game = default_game();
    let plain = solve(
        &game.board,
        game.sequence.as_slice(),
        &SolveConfig::new(8, 25),
        shipped(),
    );
    for band in [1u32, 5, 25] {
        let (traced_result, trace) = traced(&game, 8, 25, band);
        assert_eq!(traced_result.packed(), plain.packed(), "band {band} changed the result");
        assert_eq!(traced_result.moves, plain.moves, "band {band} changed the Move list");
        assert_invariants(&trace, band);
    }
    // band 0 is a legal "no trace": the solve still answers, nothing is recorded.
    let (traced_result, trace) = traced(&game, 8, 25, 0);
    assert_eq!(traced_result.packed(), plain.packed());
    assert!(trace.is_empty(), "band 0 records nothing");
}

// ── the record layout ──────────────────────────────────────────────────────

#[test]
fn layer_zero_is_the_root_board_and_nothing_else() {
    let game = default_game();
    let (result, trace) = traced(&game, 8, 25, BAND);
    assert_eq!(result.status, Status::Ok);
    let root = trace.records[0];
    assert_eq!(root.layer, 0);
    assert_eq!(root.rank, 0);
    assert_eq!(root.parent_rank, -1, "§4.4: -1 at layer 0");
    assert_eq!(root.lines, 0, "nothing has been placed at the root");
    assert_eq!(
        root.score as i64,
        shipped().score(&game.board, &PlacementCtx::NONE),
        "the root is the start position, scored with no placement"
    );
    assert_eq!(counts(&trace)[0], 1, "layer 0 is one record: the root");
}

#[test]
fn every_layer_is_the_survive_set_of_that_layer_in_score_order() {
    let game = default_game();
    let (_, trace) = traced(&game, 8, 25, BAND);
    assert_invariants(&trace, BAND);

    let layers = layers(&trace);
    assert!(
        layers.len() >= 2,
        "the default game is traced layer by layer, got {} layers",
        layers.len()
    );
    // The beam never keeps more than its width, so no layer can exceed either
    // bound.
    for (index, layer) in layers.iter().enumerate().skip(1) {
        assert!(
            layer.len() <= 25 && layer.len() <= BAND as usize,
            "layer {index} keeps {} records; the beam is 25 and the band is {BAND}",
            layer.len()
        );
    }
    assert_eq!(
        counts(&trace).iter().sum::<usize>(),
        trace.len(),
        "the per-layer counts add up to the record count"
    );
}

#[test]
fn a_narrow_band_clips_records_and_says_so_with_the_unrecorded_parent_sentinel() {
    let game = default_game();
    let (_, wide) = traced(&game, 8, 25, BAND);
    let (_, narrow) = traced(&game, 8, 25, 4);
    assert_invariants(&narrow, 4);

    let wide_counts = counts(&wide);
    let narrow_counts = counts(&narrow);
    assert_eq!(narrow_counts.len(), wide_counts.len(), "clipping removes records, not layers");
    for (layer, (narrow_count, wide_count)) in narrow_counts.iter().zip(&wide_counts).enumerate() {
        assert_eq!(*narrow_count, (*wide_count).min(4), "layer {layer} keeps its top 4");
    }
    let sentinels = narrow
        .records
        .iter()
        .filter(|record| record.parent_rank == -2)
        .count();
    assert!(
        sentinels > 0,
        "a band narrower than the beam must name `recorded but not kept` parents, \
         or the parent invariant would have to lie"
    );
    // The clipped records are the same states in the same order: the prefix.
    let wide_layers = layers(&wide);
    let narrow_layers = layers(&narrow);
    for (layer, (narrow_layer, wide_layer)) in narrow_layers.iter().zip(&wide_layers).enumerate() {
        for (rank, record) in narrow_layer.iter().enumerate() {
            assert_eq!(record.score, wide_layer[rank].score, "layer {layer} rank {rank} score");
            assert_eq!(record.lines, wide_layer[rank].lines, "layer {layer} rank {rank} lines");
        }
    }
}

#[test]
fn a_trace_with_capacity_stops_at_a_layer_boundary_instead_of_overrunning() {
    let game = default_game();
    let (full_result, full) = traced(&game, 8, 25, BAND);
    let full_counts = counts(&full);
    for capacity in [0usize, 1, 5, 26, 100] {
        let mut trace = Trace::with_capacity(capacity);
        let result = solve_traced(
            &game.board,
            game.sequence.as_slice(),
            &SolveConfig::new(8, 25),
            shipped(),
            BAND,
            &mut trace,
        );
        assert_eq!(
            result.packed(),
            full_result.packed(),
            "capacity {capacity} changed the result"
        );
        assert!(
            trace.len() <= capacity,
            "capacity {capacity} was overrun: {} records",
            trace.len()
        );
        assert_invariants(&trace, BAND);
        let recorded = counts(&trace);
        // A whole number of layers from the front: half a layer would break the
        // parent invariant.
        assert_eq!(
            recorded.as_slice(),
            &full_counts[..recorded.len()],
            "capacity {capacity} recorded a partial layer"
        );
    }
}

#[test]
fn a_lost_game_is_traced_to_where_the_search_ran_out() {
    // A Goal of 40 lines in 40 pieces is out of reach on this bed, so the trace
    // runs out of pieces rather than stopping at a Goal: the last layer is where
    // the search was when it ran out.
    let game = default_game();
    let (result, trace) = traced(&game, 40, 25, BAND);
    assert_eq!(result.status, Status::Ok);
    assert!(!result.won, "goal 40 lines in 40 pieces is out of reach");
    assert_invariants(&trace, BAND);
    assert!(trace.len() > 1, "a lost game still has a trace");
}

#[test]
fn an_out_of_domain_call_records_nothing() {
    let game = default_game();
    let mut trace = Trace::new();
    let result = solve_traced(
        &game.board,
        game.sequence.as_slice(),
        &SolveConfig::new(0, 25), // goal 0 is out of domain
        shipped(),
        25,
        &mut trace,
    );
    assert_eq!(result.status, Status::OutOfDomain);
    assert!(trace.is_empty(), "a failed solve leaves no trace behind");
}

#[test]
fn an_illegal_board_records_nothing() {
    let game = default_game();
    let mut board = Board::EMPTY;
    board.0[0] = 1 << 24; // a cell above row 0: §4.2 status 3
    let mut trace = Trace::new();
    let result = solve_traced(
        &board,
        game.sequence.as_slice(),
        &SolveConfig::new(8, 25),
        shipped(),
        25,
        &mut trace,
    );
    assert_eq!(result.status, Status::IllegalBoard);
    assert!(trace.is_empty());
}

#[test]
fn a_second_traced_solve_replaces_the_first_trace() {
    let game = default_game();
    let (_, first) = traced(&game, 8, 25, BAND);
    let mut trace = Trace::new();
    let _ = solve_traced(
        &game.board,
        game.sequence.as_slice(),
        &SolveConfig::new(8, 25),
        shipped(),
        BAND,
        &mut trace,
    );
    assert_eq!(trace.len(), first.len());
    let wide_count = trace.words()[0];
    let _ = solve_traced(
        &game.board,
        game.sequence.as_slice(),
        &SolveConfig::new(8, 25),
        shipped(),
        4,
        &mut trace,
    );
    let words = trace.words();
    assert_ne!(wide_count, words[0], "the count word must be rewritten");
    assert_eq!(words[0] as usize, (words.len() - 1) / 5, "the count word is the record count");
    assert_invariants(&trace, 4);
}
