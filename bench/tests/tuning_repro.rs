//! The run is reproducible from a committed seed (§ticket 07).
//!
//! Two runs of the same search with the same seed, the same parameters and the
//! same games must produce the same eight integers, bit for bit. That is the
//! executable form of "reproducible from a committed seed": if it fails, the
//! committed `tune_seed` does not describe the committed table.
//!
//! This is the *miniature* proof (`N=4, G=2, K=2`, four-game pool, four-game
//! validation slice) so it can run on every push. The full-run proof is
//! `bench tune --replay data/measurements/e2-tuning.json`, which re-derives the
//! committed `table_fnv` from the committed seed.

use bench::dataset::{TrainingDataset, TrainingSplit, GOAL12};
use bench::tuning::{search, CeParams};
use bench::width::Bed;

fn params() -> CeParams {
    CeParams::miniature()
}

fn dataset() -> TrainingDataset {
    TrainingDataset::load(&bench::dataset::root(), &GOAL12, &TrainingSplit::tuning())
        .expect("load the tuning games")
}

#[test]
fn the_same_seed_draws_the_same_table_twice() {
    let games = dataset();
    let first = search(&games, &Bed::GOAL12, 25, &params(), 0x5eed_1234).expect("first run");
    let second = search(&games, &Bed::GOAL12, 25, &params(), 0x5eed_1234).expect("second run");

    assert_eq!(first.weights_q16, second.weights_q16);
    assert_eq!(first.weights_decimal, second.weights_decimal);
    assert_eq!(first.table_fnv, second.table_fnv);
    assert_eq!(first.minibatch_fnv, second.minibatch_fnv);
    assert_eq!(first.iterations_run, second.iterations_run);
    assert_eq!(first.stop_reason, second.stop_reason);
}

#[test]
fn a_different_seed_draws_a_different_run() {
    let games = dataset();
    let first = search(&games, &Bed::GOAL12, 25, &params(), 1).expect("first run");
    let second = search(&games, &Bed::GOAL12, 25, &params(), 2).expect("second run");
    // The minibatches are drawn from the seed, so they must differ; the tables
    // may coincide on a four-candidate search, so only the draws are asserted.
    assert_ne!(first.minibatch_fnv, second.minibatch_fnv);
}

#[test]
fn the_search_only_ever_plays_tuning_seeds() {
    let games = dataset();
    let outcome = search(&games, &Bed::GOAL12, 25, &params(), 7).expect("a run");
    assert!(outcome.seeds_played.iter().all(|seed| *seed >= 1_000));
    assert!(!outcome.seeds_played.is_empty());
    assert_eq!(outcome.seed_list_fnv, games.seed_list_fnv());
}

#[test]
fn the_draw_order_is_the_one_the_record_claims() {
    let games = dataset();
    let outcome = search(&games, &Bed::GOAL12, 25, &params(), 7).expect("a run");
    // iteration -> game seeds (index order, without replacement) -> candidate ->
    // dimension. The first draw of the run is therefore the first game seed.
    let mut rng = bench::tuning::rng::SplitMix64::tune_stream(7);
    let expected_first = rng.bounded(params().ce_pool_len()) + params().ce_pool.0;
    assert_eq!(outcome.per_iteration_seeds[0][0], expected_first);
    assert_eq!(
        outcome.per_iteration_seeds.len(),
        outcome.iterations_run as usize
    );
    for seeds in &outcome.per_iteration_seeds {
        assert_eq!(seeds.len(), params().games_per_candidate as usize);
        let mut sorted = seeds.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), seeds.len(), "a minibatch drew a game twice");
    }
}
