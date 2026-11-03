//! `data/measurements/e2-tuning.json` is the only home of the tuned table.
//!
//! Ticket 07 committed that table as a constant beside `E1_PUBLISHED`; ticket 08's
//! ladder made it lose (955 games to 956, an exact McNemar tie) and §9.4's rule
//! deleted it from the source — "the losing architecture is removed from the source,
//! not disabled by a flag". So the constant is gone, the record is what remains, and
//! these tests moved with it: they read the table from the record, assert that the
//! engine no longer carries a second shippable table, and keep the reproducibility
//! ticket 07 promised (the Q16 vector is the rounding of the committed decimals, and
//! its fingerprint describes it).
//!
//! The template is unchanged: the run's seed, Split, bed and width are the committed
//! ones — the table was tuned on the tuning Split, at the measured default width,
//! and nothing else — and E2 is a *configuration swap* on E1, the same Features, the
//! same trait, the same type, one table apart.

use std::path::{Path, PathBuf};

use bench::json::Value;
use bench::tuning::record::{self, Record};
use bench::width;
use engine::{DellacherieThiery, DtWeights, Evaluator, E1_PUBLISHED};

fn measurements() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("bench/ has a parent")
        .join("data")
        .join("measurements")
}

fn load() -> Record {
    let path = measurements().join("e2-tuning.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "{} is not committed: ticket 07's run artifact is what ticket 08 reads ({error})",
            path.display()
        )
    });
    Record::parse(&text).expect("the record parses")
}

/// The tuned table, as the ladder builds it: the accepted run's `weights_q16`. The
/// engine has no constant for it any more, so every test here reads it the way the
/// ladder does.
fn tuned(record: &Record) -> DtWeights {
    let accepted = record
        .accepted
        .as_ref()
        .expect("an accepted run is what makes a tuned table");
    let run = record
        .runs
        .iter()
        .find(|run| run.run == accepted.run)
        .expect("the accepted run is in the record");
    DtWeights::from_array(run.weights_q16)
}

fn width_curve() -> Value {
    let path = measurements().join("width-curve.json");
    let text = std::fs::read_to_string(&path).expect("read the width curve");
    bench::json::parse(&text).expect("parse the width curve")
}

// ----------------------------------------------------------------- the table

/// Ticket 07: "E2 is a configuration swap on E1, not a third Evaluator."
#[test]
fn e2_is_the_same_type_and_the_same_features_as_e1() {
    let e2 = tuned(&load());
    let e1: DtWeights = E1_PUBLISHED;
    assert_eq!(e1.as_array().len(), e2.as_array().len());
    assert_eq!(DtWeights::FEATURES.len(), 8);
    // Both are the one `DellacherieThiery` type, so the table is what differs.
    let one = DellacherieThiery(E1_PUBLISHED);
    let two = DellacherieThiery(e2);
    assert_eq!(engine::Evaluator::name(&one), engine::Evaluator::name(&two));
    assert_ne!(
        e1.as_array(),
        e2.as_array(),
        "the tuned table is not a copy of E1"
    );
}

/// There is exactly one Dellacherie–Thiery evaluator in the engine, so E2 cannot
/// have grown a second scoring path.
#[test]
fn the_engine_has_one_dellacherie_thiery_evaluator() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("bench/ has a parent")
        .join("engine")
        .join("src")
        .join("evaluator.rs");
    let text = std::fs::read_to_string(&source).expect("read the evaluator");
    assert_eq!(text.matches("pub struct DellacherieThiery").count(), 1);
    assert_eq!(
        text.matches("impl Evaluator for DellacherieThiery").count(),
        1
    );
    // One table, and it is the published one: the tuned table lost ticket 08's
    // ladder and §9.4 deleted it from the source, so no constant switches the
    // shipped answer.
    assert_eq!(text.matches("pub const E1_PUBLISHED: DtWeights").count(), 1);
}

/// §9.4's other half, and the reason this file changed: the losing table is not in
/// the engine at all.
///
/// Deleting it is only half the rule; the other half is that the evidence survives.
/// So this asserts both: no `E2_TUNED` anywhere in `engine/src/evaluator.rs`, and the
/// weights still in the record the ladder reads them from.
#[test]
fn the_losing_table_is_not_in_the_engine() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("bench/ has a parent")
        .join("engine")
        .join("src")
        .join("evaluator.rs");
    let text = std::fs::read_to_string(&source).expect("read the evaluator");
    assert!(
        !text.contains("E2_TUNED"),
        "the tuned table is back in the engine: it lost the ladder 955 to 956 and §9.4 \
         deletes what the ladder rejects rather than leaving it one match from shipping"
    );
    // The deletion loses nothing: the eight integers are the record's, and the
    // record's fingerprint describes them.
    let record = load();
    let run = record
        .runs
        .iter()
        .find(|run| run.weights_q16 == tuned(&record).as_array())
        .expect("the accepted run's vector");
    assert_eq!(run.weights_q16.len(), 8);
    assert_eq!(
        bench::tuning::table_fnv(&run.weights_q16),
        run.table_fnv,
        "the record's fingerprint does not describe the vector it holds"
    );
}

/// §9.4's ship rule is ticket 08's: 07 only makes the table selectable.
#[test]
fn shipping_is_unchanged_by_the_tuning_run() {
    let shipped = engine::shipped();
    assert_eq!(shipped.name(), "dellacherie-thiery");
    let e1 = DellacherieThiery(E1_PUBLISHED);
    let board = engine::Board([0u32; 10]);
    assert_eq!(
        shipped.score(&board, &engine::PlacementCtx::NONE),
        e1.score(&board, &engine::PlacementCtx::NONE)
    );
}

// ------------------------------------------------------------------ the record

#[test]
fn the_record_is_on_the_tuning_split_at_the_measured_default_width() {
    let record = load();
    let curve = width_curve();
    let default_beam = curve
        .get("default_beam")
        .and_then(Value::as_u32)
        .expect("the width curve names its default");
    assert!(!record.runs.is_empty(), "the record has no runs");
    for run in &record.runs {
        assert_eq!(run.split, "1000-1999");
        assert_eq!(run.width, default_beam, "the run is not at DEFAULT_BEAM");
        assert_eq!(run.seed_list_fnv, "0x45ff8c50e8139615");
        assert_eq!(run.bed.goal, 12);
        assert_eq!(run.bed.pieces, 100);
        assert_eq!(run.bed.cap, 7);
        assert_eq!(run.bed.name, "goal12-p100-cap7");
        // §6.1's fingerprint is the committed one, so the games are the games.
        assert_eq!(run.dataset_fingerprint, "4ebdf44940134c40");
        assert_eq!(run.weights_q16.len(), 8);
        assert_eq!(run.weights_decimal.len(), 8);
    }
}

#[test]
fn the_q16_table_is_re_derived_from_the_decimals() {
    let record = load();
    for run in &record.runs {
        let re_derived = bench::tuning::q32::to_q16(&run.weights_decimal);
        assert_eq!(
            re_derived, run.weights_q16,
            "run {}: the committed Q16 vector is not the rounding of its decimals",
            run.run
        );
        assert_eq!(
            bench::tuning::table_fnv(&run.weights_q16),
            run.table_fnv,
            "run {}: the table fingerprint does not describe the committed vector",
            run.run
        );
    }
}

/// The record names its accepted run and nothing else does, so "which table" is a
/// field rather than an inference.
#[test]
fn the_record_names_the_accepted_run_and_nothing_else_does() {
    let record = load();
    match &record.accepted {
        Some(accepted) => {
            let run = record
                .runs
                .iter()
                .find(|run| run.run == accepted.run)
                .expect("the accepted run exists");
            assert_eq!(run.decision, "accepted");
            assert_eq!(run.rejection_kind, None);
            assert_eq!(
                record
                    .runs
                    .iter()
                    .filter(|run| run.decision == "accepted")
                    .count(),
                1,
                "two accepted runs would leave the tuned table ambiguous"
            );
            assert_eq!(tuned(&record).as_array(), run.weights_q16);
        }
        None => {
            // No run cleared the rule: the newest table is committed as the
            // candidate and the record says, in one field, that it did not win.
            let last = record.runs.last().expect("a run");
            assert_eq!(last.decision, "rejected");
            assert!(last.rejection_kind.is_some());
            assert!(last.rejection_reason.is_some());
        }
    }
}

/// A rejection is not a footnote: the record files it under fields a reader meets
/// first, and the reason is words rather than a code.
///
/// This used to read the deleted constant's doc comment. With the constant gone, the
/// record is the only place the rejection can live, which is the point of keeping it:
/// §9.4 deletes the table and keeps the evidence.
#[test]
fn a_rejection_is_visible_in_the_record() {
    let record = load();
    for run in &record.runs {
        match run.decision.as_str() {
            "accepted" => {
                assert_eq!(
                    run.rejection_kind, None,
                    "run {}: accepted and rejected",
                    run.run
                );
                assert_eq!(run.rejection_reason, None);
            }
            "rejected" => {
                let kind = run
                    .rejection_kind
                    .as_deref()
                    .unwrap_or_else(|| panic!("run {} was rejected and does not say why", run.run));
                assert!(!kind.is_empty(), "run {}: an empty rejection kind", run.run);
                let reason = run
                    .rejection_reason
                    .as_deref()
                    .unwrap_or_else(|| panic!("run {}: a rejection without a sentence", run.run));
                assert!(
                    reason.len() > kind.len(),
                    "run {}: the rejection reason is not a sentence about the kind",
                    run.run
                );
            }
            other => panic!("run {}: {other} is not a decision", run.run),
        }
    }
    if record.accepted.is_none() {
        let last = record.runs.last().expect("a run");
        assert_eq!(last.decision, "rejected", "no run was accepted");
    }
}

#[test]
fn every_per_seed_row_is_a_tuning_seed_at_the_default_width() {
    let path = measurements().join("e2-tuning-rows.jsonl");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "{} is not committed: ticket 08 compares these rows, it does not retrain ({error})",
            path.display()
        )
    });
    let record = load();
    let mut seen = std::collections::BTreeSet::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let row = record::Row::parse(line).expect("a row parses");
        assert!(
            (1_000..=1_999).contains(&row.seed),
            "row seed {} is outside the tuning Split",
            row.seed
        );
        assert_eq!(row.width, record.runs.last().expect("a run").width);
        assert_eq!(row.bed, "goal12-p100-cap7");
        assert!(row.evals > 0);
        assert!(row.moves <= 100);
        if row.won {
            // A win stops the Solve at the Goal, but the placement that crosses
            // it can clear up to three lines past it (a 4-line clear from 11
            // reaches 15), so `lines` is `goal..=goal+3` on a win -- which is
            // also why a mean of lines can exceed the Goal.
            assert!(
                (12..=15).contains(&row.lines),
                "seed {} won with {} lines",
                row.seed,
                row.lines
            );
        }
        assert!(seen.insert((row.run, row.evaluator.clone(), row.seed)));
    }
    // Four tables over all 1,000 tuning seeds, for the newest run.
    let newest = record.runs.last().expect("a run").run;
    let count = seen.iter().filter(|(run, _, _)| *run == newest).count();
    assert_eq!(
        count,
        4 * 1_000,
        "the rows are not four tables over 1,000 seeds"
    );
    for evaluator in ["e0", "e1", "bcts", "e2"] {
        let n = seen
            .iter()
            .filter(|(run, name, _)| *run == newest && name == evaluator)
            .count();
        assert_eq!(n, 1_000, "{evaluator} does not cover the tuning Split");
    }
}

#[test]
fn the_record_carries_the_paired_statistics_the_ladder_needs() {
    let record = load();
    for run in &record.runs {
        for (name, stats) in &run.stats {
            assert_eq!(stats.games, 1_000, "{name} is not measured on 1,000 games");
            assert!(
                (0.0..=1.0).contains(&stats.win_rate),
                "{name}: a win rate is not a rate"
            );
            assert!(stats.mean_lines >= 0.0 && stats.mean_lines <= 15.0);
            assert!(stats.evals_median > 0.0);
        }
        let comparison = &run.comparison;
        assert!(comparison.e2_vs_e1.b <= 1_000 && comparison.e2_vs_e1.c <= 1_000);
        assert!((0.0..=1.0).contains(&comparison.e2_vs_e1.mcnemar_p));
        let [low, high] = comparison.e2_vs_e1.mean_lines_ci95;
        assert!(low <= high);
        // The win-rate axis is named, not left to a reader's arithmetic.
        assert!(matches!(
            comparison.win_rate_axis.as_str(),
            "resolved" | "no-resolution"
        ));
    }
}

#[test]
fn the_width_the_run_used_is_the_width_curve_it_named() {
    let record = load();
    let curve = width_curve();
    let default_beam = curve
        .get("default_beam")
        .and_then(Value::as_u32)
        .expect("default_beam");
    let basis = curve
        .get("default_beam_basis")
        .and_then(Value::as_str)
        .expect("default_beam_basis");
    for run in &record.runs {
        assert_eq!(run.width, default_beam);
        assert_eq!(run.width_curve.default_beam, default_beam);
        assert_eq!(run.width_curve.default_beam_basis, basis);
        assert_eq!(run.width_curve.path, "data/measurements/width-curve.json");
    }
    // And the trainer reads it, rather than carrying a constant.
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("tuning")
        .join("mod.rs");
    let text = std::fs::read_to_string(&source).expect("read the trainer");
    assert!(
        text.contains("width-curve.json") || text.contains("width_curve"),
        "the trainer does not read the width curve"
    );
    let _ = width::TUNING_SPLIT;
}

/// The reader and the writer must agree on every byte.
///
/// This is the test that would have caught the first record this ticket wrote: a
/// trailing comma after the last field of a table's stats object made the file
/// unparseable, and only `bench tune --replay` noticed. A renderer that cannot
/// re-read its own output is a record nobody can audit.
#[test]
fn the_record_round_trips_byte_for_byte() {
    let path = measurements().join("e2-tuning.json");
    let text = std::fs::read_to_string(&path).expect("read the record");
    // The escapes, not literal CR/LF bytes: `core.autocrlf` rewrites a
    // literal CRLF inside the source when the file is checked out, which
    // turns this normalisation into a no-op and makes the comparison fail on
    // line endings alone. `render()` writes LF, the committed blob is LF, and
    // a Windows worktree is CRLF.
    let normalised = text.replace("\r\n", "\n");
    let record = Record::parse(&normalised).expect("the record parses");
    assert_eq!(
        record.render(),
        normalised,
        "render(parse(text)) is not text: the record would drift on the next append"
    );
}

/// The same for one row line.
#[test]
fn a_row_round_trips_byte_for_byte() {
    let path = measurements().join("e2-tuning-rows.jsonl");
    let text = std::fs::read_to_string(&path).expect("read the rows");
    for line in text.lines().filter(|line| !line.trim().is_empty()).take(20) {
        let row = record::Row::parse(line).expect("a row parses");
        assert_eq!(row.render(), line);
    }
}
