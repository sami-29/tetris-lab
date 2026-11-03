//! Ticket 08's ladder, tested without a 1,000-game measurement: the decision
//! rules are pure functions, and one tiny bed proves the plumbing writes an
//! artifact that names what it measured.
//!
//! The measurement itself (`data/measurements/ablation-ladder.json`) is the run's
//! receipt; these tests are the rules it was read by.

use bench::ladder::{
    self, Candidate, ScanRow, ToggleVerdict, BOOTSTRAP_RESAMPLES, SCAN_CAP, SCAN_PIECES,
};
use bench::tuning::stats::Sample;
use bench::width::Paired;

// ------------------------------------------------------------------ fixtures

/// `total` seed-ordered samples, the first `wins` of which won at `goal`.
fn samples(wins: usize, total: usize, goal: u32) -> Vec<Sample> {
    (0..total)
        .map(|index| Sample {
            seed: 1_000 + index as u32,
            won: index < wins,
            lines: if index < wins { goal } else { goal - 1 },
            evals: 10_000 + index as u32,
            moves: 40,
            ms: 1.0,
            pieces: 100,
        })
        .collect()
}

/// The same samples with every evaluation count shifted, i.e. a cost-only change.
fn with_evals(mut rows: Vec<Sample>, evals: u32) -> Vec<Sample> {
    for row in rows.iter_mut() {
        row.evals = evals;
    }
    rows
}

fn candidate(id: &'static str, features: u32, published: bool, samples: Vec<Sample>) -> Candidate {
    // E0 is a different architecture from E1/E2, which is what scopes §9.4's
    // published-over-tuned rule to the pair it names.
    let architecture = if id == "e0" {
        "heuristic"
    } else {
        "dellacherie-thiery"
    };
    Candidate {
        id,
        table: "TEST",
        features,
        published,
        architecture,
        samples,
    }
}

fn scans(rows: &[(u32, u32)]) -> Vec<ScanRow> {
    rows.iter()
        .map(|(goal, wins)| ScanRow {
            goal: *goal,
            wins: *wins,
            games: 1_000,
            mean_lines: *goal as f64,
            evals_mean: 1_000.0,
        })
        .collect()
}

// --------------------------------------------------------------- the goal scan

#[test]
fn the_scan_stops_at_the_first_goal_below_ninety_nine_percent() {
    // 990 of 1,000 is exactly 99 %, so it is still saturated: the rule is
    // `wins * 100 >= games * 99`, never a rounded percentage.
    let rows = scans(&[(16, 1_000), (64, 1_000), (65, 940), (66, 569)]);
    assert_eq!(ladder::first_goal_below_saturation(&rows), Some(2));
    let rows = scans(&[(16, 1_000), (17, 990), (18, 989)]);
    assert_eq!(ladder::first_goal_below_saturation(&rows), Some(2));
    let rows = scans(&[(16, 1_000), (17, 1_000)]);
    assert_eq!(ladder::first_goal_below_saturation(&rows), None);
}

#[test]
fn the_derived_bed_is_the_first_sub_99_goal_or_a_refusal() {
    let rows = scans(&[(16, 1_000), (65, 940), (69, 0), (70, 0)]);
    let bed = ladder::derive_bed(&rows, SCAN_PIECES, SCAN_CAP).expect("a bed");
    assert_eq!(bed.goal, 65);
    assert_eq!(bed.pieces, SCAN_PIECES);
    assert_eq!(bed.cap, SCAN_CAP);
    assert_eq!(bed.name(), "goal65-p160-cap7");

    // No sub-99 % goal: the protocol says stop and report, never invent a bed.
    let rows = scans(&[(16, 1_000), (64, 1_000)]);
    let error = ladder::derive_bed(&rows, SCAN_PIECES, SCAN_CAP).expect_err("a refusal");
    assert!(error.contains("99"), "{error}");
    assert!(error.contains("160"), "{error}");
}

#[test]
fn the_scan_ends_when_two_goals_in_a_row_are_unreachable() {
    let rows = scans(&[(65, 940), (68, 3), (69, 0)]);
    assert!(!ladder::scan_is_done(&rows), "one zero is not the end");
    let rows = scans(&[(68, 3), (69, 0), (70, 0)]);
    assert!(ladder::scan_is_done(&rows));
    assert_eq!(ladder::unreachable_from(&rows), Some(69));
}

#[test]
fn the_line_ceiling_is_the_cells_the_pieces_can_fill() {
    assert_eq!(ladder::line_ceiling(SCAN_PIECES), 64);
    assert_eq!(ladder::line_ceiling(100), 40);
}

// ------------------------------------------------------------------- the cascade

#[test]
fn the_cascade_ships_the_highest_win_rate_when_the_difference_is_significant() {
    let rows = vec![
        candidate("e0", 4, false, samples(900, 1_000, 65)),
        candidate("e1", 8, true, samples(1_000, 1_000, 65)),
        candidate("e2", 8, false, samples(950, 1_000, 65)),
    ];
    let verdict = ladder::cascade(&rows).expect("a verdict");
    assert_eq!(verdict.winner, "e1");
    assert_eq!(verdict.leg, ladder::Leg::WinRate);
    assert_eq!(verdict.tied, vec!["e1"]);
}

#[test]
fn a_p_value_at_or_above_one_percent_is_a_tie() {
    // 7 discordant seeds, all one way: p = 2/128 = 0.0156 >= 0.01, a tie.
    let rows = vec![
        candidate("e1", 8, true, samples(1_000, 1_000, 65)),
        candidate("e2", 8, false, samples(993, 1_000, 65)),
    ];
    let verdict = ladder::cascade(&rows).expect("a verdict");
    assert_eq!(verdict.tied, vec!["e1", "e2"]);

    // 8 discordant seeds: p = 2/256 = 0.0078 < 0.01, a real difference.
    let rows = vec![
        candidate("e1", 8, true, samples(1_000, 1_000, 65)),
        candidate("e2", 8, false, samples(992, 1_000, 65)),
    ];
    let verdict = ladder::cascade(&rows).expect("a verdict");
    assert_eq!(verdict.leg, ladder::Leg::WinRate);
    assert_eq!(verdict.winner, "e1");
}

#[test]
fn a_tie_goes_to_fewer_evaluations_per_solve() {
    let rows = vec![
        candidate("e0", 4, false, with_evals(samples(1_000, 1_000, 65), 20_000)),
        candidate("e1", 8, true, with_evals(samples(1_000, 1_000, 65), 18_000)),
    ];
    let verdict = ladder::cascade(&rows).expect("a verdict");
    assert_eq!(verdict.winner, "e1");
    assert_eq!(verdict.leg, ladder::Leg::Evaluations);
}

#[test]
fn an_evaluations_tie_goes_to_fewer_features() {
    let rows = vec![
        candidate("e1", 8, true, with_evals(samples(1_000, 1_000, 65), 18_000)),
        candidate("e0", 4, false, with_evals(samples(1_000, 1_000, 65), 18_000)),
    ];
    let verdict = ladder::cascade(&rows).expect("a verdict");
    assert_eq!(verdict.winner, "e0");
    assert_eq!(verdict.leg, ladder::Leg::Features);
}

#[test]
fn the_published_table_wins_the_last_leg() {
    // Same win rate, same evaluations, same feature count: §9.4's closing
    // sentence decides it, and it is the published table.
    let rows = vec![
        candidate("e2", 8, false, with_evals(samples(1_000, 1_000, 65), 18_000)),
        candidate("e1", 8, true, with_evals(samples(1_000, 1_000, 65), 18_000)),
    ];
    let verdict = ladder::cascade(&rows).expect("a verdict");
    assert_eq!(verdict.winner, "e1");
    assert_eq!(verdict.leg, ladder::Leg::Published);
}

#[test]
fn a_fewer_evaluations_nudge_does_not_displace_the_published_table() {
    // The measured shape of ticket 08's bed: E1 and E2 tie on win rate and on
    // feature count, and E2's evaluation median is a hair lower (24 out of 90,000).
    // §9.4's closing sentence — "E1 ships unless E2 clears p < 0.01" — is applied
    // before the legs for exactly this pair, so the nudge cannot ship a tuned table.
    let rows = vec![
        candidate("e1", 8, true, with_evals(samples(956, 1_000, 65), 89_959)),
        candidate("e2", 8, false, with_evals(samples(955, 1_000, 65), 89_935)),
    ];
    let verdict = ladder::cascade(&rows).expect("a verdict");
    assert_eq!(verdict.winner, "e1");
    assert_eq!(verdict.leg, ladder::Leg::Published);

    // But a tuned table that *does* clear p < 0.01 on win rate displaces it.
    let rows = vec![
        candidate("e1", 8, true, with_evals(samples(956, 1_000, 65), 89_959)),
        candidate("e2", 8, false, with_evals(samples(964, 1_000, 65), 100_000)),
    ];
    let verdict = ladder::cascade(&rows).expect("a verdict");
    assert_eq!(verdict.winner, "e2");
    assert_eq!(verdict.leg, ladder::Leg::WinRate);
}

#[test]
fn the_published_over_tuned_rule_cannot_retire_a_different_architecture() {
    // E0 is the Ablation control, not a tuned DT table: the rule is scoped by
    // architecture, so a win-rate tie against the published table still goes to
    // the legs — and here the control's 4 features win on the last one.
    let rows = vec![
        candidate("e1", 8, true, with_evals(samples(1_000, 1_000, 65), 18_000)),
        candidate("e0", 4, false, with_evals(samples(1_000, 1_000, 65), 18_000)),
    ];
    let verdict = ladder::cascade(&rows).expect("a verdict");
    assert_eq!(verdict.winner, "e0");
    assert_eq!(verdict.leg, ladder::Leg::Features);
}

// ------------------------------------------------------------------- the toggles

#[test]
fn a_toggle_that_moves_nothing_is_deleted() {
    assert_eq!(
        ladder::toggle_verdict(true, 0, Paired::default(), [0.0, 0.0]),
        ToggleVerdict::Deleted,
        "a mechanism that changed not one number on 1,000 games has nothing to keep"
    );
    assert_eq!(
        ladder::toggle_verdict(false, 0, Paired::default(), [0.0, 0.0]),
        ToggleVerdict::Deleted
    );
}

#[test]
fn a_toggle_that_moves_something_outside_its_interval_is_kept() {
    // Either statistic clears: a win flip the exact test rejects …
    assert_eq!(
        ladder::toggle_verdict(true, 20, Paired { b: 12, c: 0 }, [-0.01, 0.01]),
        ToggleVerdict::Kept
    );
    // … or a mean-lines interval that excludes zero.
    assert_eq!(
        ladder::toggle_verdict(true, 20, Paired { b: 3, c: 1 }, [0.002, 0.020]),
        ToggleVerdict::Kept
    );
}

// --------------------------------------------------------------- the bootstrap

#[test]
fn the_bootstrap_seed_is_a_function_of_the_artifacts_own_fields() {
    let one = ladder::bootstrap_seed("goal65-p160-cap7", 25, "e0-vs-e1", "4ebdf44940134c40");
    let again = ladder::bootstrap_seed("goal65-p160-cap7", 25, "e0-vs-e1", "4ebdf44940134c40");
    assert_eq!(one, again, "a re-run reproduces the interval");
    // A different pair, width or dataset is a different measurement, so it gets a
    // different stream on purpose.
    assert_ne!(
        one,
        ladder::bootstrap_seed("goal65-p160-cap7", 25, "e0-vs-e2", "4ebdf44940134c40")
    );
    assert_ne!(
        one,
        ladder::bootstrap_seed("goal65-p160-cap7", 50, "e0-vs-e1", "4ebdf44940134c40")
    );
    assert_ne!(
        one,
        ladder::bootstrap_seed("goal65-p160-cap7", 25, "e0-vs-e1", "0000000000000000")
    );
    assert_eq!(BOOTSTRAP_RESAMPLES, 10_000);
}

#[test]
fn a_pair_reports_the_two_by_two_table_and_a_reproducible_interval() {
    // `judged` first, `reference` second: the challenger against the winner, which
    // is the direction every row of the published table is read in.
    let judged = candidate("e0", 4, false, samples(940, 1_000, 65));
    let reference = candidate("e1", 8, true, samples(1_000, 1_000, 65));
    let seed = ladder::bootstrap_seed("goal65-p160-cap7", 25, "e0-vs-e1", "fixture");
    let pair = ladder::compare(&judged, &reference, seed);
    // 60 seeds differ, all in E0's favour, and the identity b - c = win difference
    // is what a reader checks the table against.
    assert_eq!((pair.only_judged, pair.only_reference), (0, 60));
    assert_eq!(pair.both, 940);
    assert_eq!(pair.neither, 0);
    assert_eq!(pair.discordant, Paired { b: 0, c: 60 });
    assert!(pair.rejects, "60 discordant seeds reject at p < 0.01");
    assert!(pair.delta_mean_lines < 0.0, "E0 is the worse one here");
    // The interval is a function of the pinned stream: same seed, same bytes.
    let again = ladder::compare(&judged, &reference, seed);
    assert_eq!(pair.ci95, again.ci95);
    assert_eq!(pair.boot_seed, seed);
}

// ------------------------------------------------------------------ the artifact

#[test]
fn a_ladder_run_writes_an_artifact_that_names_what_it_measured() {
    let dir = std::env::temp_dir().join(format!("ladder-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a temp dir");

    let bed = bench::width::Bed {
        goal: 4,
        pieces: 20,
        cap: 7,
    };
    let options = ladder::Options {
        out: dir.join("ablation-ladder.json"),
        rows_path: dir.join("ablation-ladder-rows.jsonl"),
        docs_path: dir.join("ablation-ladder.md"),
        bed: Some(bed),
        seeds: (1_000, 1_023),
        width: 3,
        width_cap: 800,
        quality_fingerprint: "fnv1a64:fixture".to_string(),
        machine: "test box".to_string(),
        engine_commit: "fixture".to_string(),
        date: "2026-11-02T00:00:00Z".to_string(),
        ..ladder::Options::defaults()
    };
    let outcome = ladder::run(&options).expect("a finished run");

    // One row per rung: three evaluators plus three toggles off the winner.
    assert_eq!(outcome.artifact.rows.len(), 6, "{:?}", outcome.artifact.rows);
    let ids: Vec<&str> = outcome
        .artifact
        .rows
        .iter()
        .map(|row| row.id)
        .collect();
    assert_eq!(ids, vec!["e0", "e1", "e2", "dedup-off", "tail-off", "no-suicide-off"]);
    // Every rung solved the same games, so the pairing is real.
    for row in &outcome.artifact.rows {
        assert_eq!(row.stats.games, 24, "{}", row.id);
    }
    // The verdict is one of the three tables and the cascade names its leg.
    assert!(ids.contains(&outcome.artifact.verdict.winner));

    let text = std::fs::read_to_string(&options.out).expect("the artifact");
    for key in [
        "\"kind\": \"ablation-ladder\"",
        "\"bed_source\": \"given\"",
        "\"winner\"",
        "\"cascade\"",
        "\"toggles\"",
        "\"labels\"",
        "goal4-p20-cap7",
    ] {
        assert!(text.contains(key), "the artifact is missing {key}:\n{text}");
    }
    // The per-seed rows are the artifact's evidence, one line per (row, seed).
    let rows = std::fs::read_to_string(&options.rows_path).expect("the rows");
    assert_eq!(rows.lines().count(), 6 * 24);
    assert!(rows.contains("\"row\":\"e1\""));
    // The published table names the bed, the Split and the machine.
    let table = std::fs::read_to_string(&options.docs_path).expect("the table");
    assert!(table.contains("goal4-p20-cap7"), "{table}");
    assert!(table.contains("1,000–1,023"), "{table}");
    assert!(table.contains("test box"), "{table}");

    let _ = std::fs::remove_dir_all(&dir);
}
