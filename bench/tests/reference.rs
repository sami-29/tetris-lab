//! Ticket 09: the Reference run's row shape, its statistics, and the append-only
//! rule that protects a committed row (§9.6).
//!
//! Nothing here solves a thousand games. `Measurement::from_vectors` builds exactly
//! what a measured run produces — per-seed evaluations, lines, wins and a clock —
//! so the arithmetic, the fingerprints and the JSON a reader and the page depend on
//! are checked in milliseconds. The numbers themselves are measured by
//! `bench reference` and pinned by `data/measurements/golden-evals.json`.

use std::fs;
use std::path::PathBuf;

use bench::dataset::{self, Config, CONFIGS, GOAL8};
use bench::fingerprint::{self, Fnv};
use bench::json::{self, Value};
use bench::reference::{
    self, DatasetId, Engine, Machine, Measurement, RunLabels, Split,
};

// ---------------------------------------------------------------- fixtures

fn engine() -> Engine {
    Engine {
        // 40 hex, the full commit: the label a reader checks out.
        commit: "0123456789abcdef0123456789abcdef01234567".to_string(),
        short: "0123456".to_string(),
        dirty: false,
        wasm_sha256: "3cb88e38f8d497aa2b080d65386ec92ef0eafec96760e757fb18cd347a473022".to_string(),
        wasm_bytes: 58_302,
        profile: reference::ENGINE_PROFILE.to_string(),
        toolchain: "1.99.1-x86_64-pc-windows-gnu".to_string(),
    }
}

fn machine() -> Machine {
    Machine {
        cpu: "AMD Ryzen 7 5700X 8-Core Processor (8C/16T), 31.9 GB".to_string(),
        os: "Microsoft Windows 11 Pro 10.0.22621".to_string(),
        runtime: "node v24.21.0 (V8 headless, not a browser: no DOM, one thread)".to_string(),
        substrate: "wasm".to_string(),
        browser_or_node: "node 24.21 (V8 headless), not a browser".to_string(),
        rustc: "rustc 1.99.1 (58cf8f9c3 2026-10-13)".to_string(),
        node: "v24.21.0".to_string(),
    }
}

fn labels(first_index: u32, first_supersedes: Option<String>) -> RunLabels {
    RunLabels {
        engine: engine(),
        machine: machine(),
        datasets: vec![
            (
                GOAL8.dir.to_string(),
                DatasetId {
                    fingerprint: "4bf1a17b1685a774".to_string(),
                    generator_version: 1,
                },
            ),
        ],
        measured_at: "2026-11-03T02:30:00Z".to_string(),
        reason: "test row: no measurement was run to make it".to_string(),
        first_index,
        first_supersedes,
    }
}

fn measurement(evals: &[u32], lines: &[u32]) -> Measurement {
    Measurement::from_vectors(
        GOAL8,
        25,
        Split {
            first: 0,
            last: (evals.len() as u32) - 1,
        },
        evals,
        lines,
        2_395.53,
        747.34,
    )
}

fn temp_dir(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("tetris-lab-09-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a temp dir");
    dir
}

// -------------------------------------------------------------- statistics

#[test]
fn the_wilson_interval_is_the_published_formula_and_is_exact_at_saturation() {
    // Reproducing notes/09 §1.2's checked values: no continuity correction, no
    // bootstrap, and a saturated bed gives an upper bound of exactly 1.
    let saturated = reference::wilson95(1000, 1000);
    assert!(
        (saturated[0] - 0.996_173).abs() < 5e-7,
        "{saturated:?}"
    );
    assert_eq!(saturated[1], 1.0, "a saturated proportion is exactly 1");

    let nearly = reference::wilson95(995, 1000);
    assert!((nearly[0] - 0.988_349).abs() < 5e-7, "{nearly:?}");
    assert!((nearly[1] - 0.997_862).abs() < 5e-7, "{nearly:?}");

    let small = reference::wilson95(200, 200);
    assert!((small[0] - 0.981_155).abs() < 5e-7, "{small:?}");
    assert_eq!(small[1], 1.0);

    // Nothing won: the lower bound is exactly 0, not a negative number.
    let none = reference::wilson95(0, 1000);
    assert_eq!(none[0], 0.0);
    assert!((none[1] - 0.003_827).abs() < 5e-7, "{none:?}");
}

#[test]
fn the_percentiles_are_nearest_rank_and_an_absent_sample_is_null() {
    // Ten samples, eight wins: two failures at 5 and 3 lines. Nearest rank on
    // p50 = ceil(0.5 * 10) = 5 -> x[4], p90 = ceil(9) -> x[8].
    let sample = measurement(
        &[10, 20, 30, 40, 50, 60, 70, 80, 90, 100],
        &[8, 8, 8, 8, 8, 8, 8, 8, 5, 3],
    );
    let stats = sample.stats();
    assert_eq!(stats.n, 10);
    assert_eq!(stats.wins, 8);
    assert_eq!(stats.n_fail, 2);
    assert_eq!(stats.evals_p50, 50);
    assert_eq!(stats.evals_p90, 90);
    assert_eq!(stats.evals_max, 100);
    assert_eq!(stats.fail_lines_mean, Some(4.0));
    assert_eq!(stats.fail_lines_median, Some(3));
    assert_eq!(stats.wilson95, reference::wilson95(8, 10));

    // A saturated bed: no failures at all, so both rows are `null`, never 0. A
    // stored 0 would be the page printing a measurement that does not exist.
    let clean = measurement(&[400, 500, 600], &[8, 9, 8]);
    let stats = clean.stats();
    assert_eq!(stats.n_fail, 0);
    assert_eq!(stats.fail_lines_mean, None);
    assert_eq!(stats.fail_lines_median, None);
    assert_eq!(stats.wins, 3);
}

#[test]
fn a_solve_that_clears_more_lines_than_the_goal_is_still_a_win() {
    // `won` is `lines >= goal`, never `lines == goal`: one placement can clear more
    // rows than were needed (measured: seed 32 at width 24 clears 9 with goal 8).
    let sample = measurement(&[1, 2], &[9, 7]);
    assert_eq!(sample.samples[0].won, true);
    assert_eq!(sample.samples[1].won, false);
}

#[test]
fn the_fingerprints_are_fnv_over_the_vectors_in_seed_order() {
    let evals = [10_212u32, 9_810, 10_340];
    let lines = [8u32, 8, 9];
    let sample = measurement(&evals, &lines);

    let mut expected = Fnv::new();
    for value in evals {
        expected.u32_le(value);
    }
    assert_eq!(sample.evals_fingerprint(), fingerprint::hex(expected.finish()));

    let mut expected = Fnv::new();
    for value in lines {
        expected.byte(value as u8);
    }
    assert_eq!(sample.lines_fingerprint(), fingerprint::hex(expected.finish()));

    // 16 lowercase hex digits, the form a human eyeballs in a failure message.
    assert_eq!(sample.evals_fingerprint().len(), 16);
    assert!(sample
        .evals_fingerprint()
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
}

// ---------------------------------------------------------------- the row

#[test]
fn a_reference_row_carries_every_field_the_page_reads() {
    let rows = reference::rows(
        &[measurement(&[10_212, 9_810, 10_340, 12_000], &[8, 8, 9, 8])],
        &labels(0, None),
    );
    let rendered = rows[0].render();

    // One compact JSON object on one line: it is a log, and `git diff` reads it.
    assert_eq!(rendered.lines().count(), 1, "a row is one line: {rendered}");
    assert!(!rendered.ends_with('\n'), "the caller writes the newline");
    let parsed = json::parse(&rendered).expect("a row is JSON");

    for (figure, paths) in reference::PAGE_FIELDS {
        for path in paths {
            let value = reference::resolve(&parsed, path).unwrap_or_else(|| {
                panic!("{figure}: the page reads {path} and the row does not carry it: {rendered}")
            });
            // `null` is a value: R3/R4 render "no failures" from it. Absence is not.
            let _ = value;
        }
    }

    // R1's CI travels with the rate: a bare 100 % on a saturated bed is misleading.
    let wilson = reference::resolve(&parsed, "stats.wilson95").unwrap();
    assert_eq!(wilson.as_array().map(|array| array.len()), Some(2));

    // R2's denominator is the split's own n, never a number the page invents.
    assert_eq!(reference::resolve(&parsed, "split.n").unwrap().as_u32(), Some(4));
    assert_eq!(
        reference::resolve(&parsed, "stats.wins").unwrap().as_u32(),
        Some(4)
    );

    // R3/R4 are `null` on a saturated bed, and still present.
    assert_eq!(
        *reference::resolve(&parsed, "stats.fail_lines_mean").unwrap(),
        Value::Null
    );
    assert_eq!(
        reference::resolve(&parsed, "stats.n_fail").unwrap().as_u32(),
        Some(0)
    );
}

#[test]
fn a_reference_row_labels_the_engine_the_box_the_runtime_and_the_date() {
    let rows = reference::rows(
        &[measurement(&[10_212, 9_810], &[8, 8])],
        &labels(0, None),
    );
    let row = &rows[0];

    assert_eq!(row.engine.commit.len(), 40, "the full commit, not a short one");
    assert_eq!(row.engine.short.len(), 7);
    assert_eq!(row.engine.wasm_sha256.len(), 64, "sha256 hex");
    assert!(
        row.engine.wasm_sha256
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "{}",
        row.engine.wasm_sha256
    );
    assert!(row.engine.profile.contains("lto=fat"), "{}", row.engine.profile);
    assert!(!row.machine.cpu.is_empty());
    assert!(!row.machine.os.is_empty());
    // §9.6 / §2.4: "browser or Node" is a label, and ours is Node — headless V8.
    assert_eq!(row.machine.substrate, "wasm");
    assert!(row.machine.runtime.contains("V8"), "{}", row.machine.runtime);
    assert!(
        row.machine.runtime.contains("not a browser"),
        "{}",
        row.machine.runtime
    );
    assert_eq!(row.measured_at, "2026-11-03T02:30:00Z");

    // The behaviour labels are the ones that make two rows comparable at all.
    assert_eq!(row.behaviour.width, 25);
    assert_eq!(row.behaviour.evaluator, reference::EVALUATOR_ID);
    assert_eq!(row.behaviour.table, reference::EVALUATOR_TABLE);
    assert!(row.behaviour.tail, "the Tail is the one remaining switch, and it ships on");
    assert_eq!(row.dataset.fingerprint, "4bf1a17b1685a774");
    assert_eq!(row.split, Split { first: 0, last: 1 });
    assert_eq!(row.run_id.matches('/').count(), 4, "{}", row.run_id);
}

#[test]
fn a_reference_row_carries_the_flat_labels_the_page_quotes() {
    // Ticket 16's Explainer does not walk the nested label block: web/scripts/
    // build-facts.mjs reads four flat strings (labels.machine, labels.runtime,
    // labels.date, labels.engine_commit) and refuses to quote a label it cannot
    // name. Ticket 09 owns this file's shape, so the row has to carry them.
    let rows = reference::rows(&[measurement(&[10_212, 9_810], &[8, 8])], &labels(0, None));
    let row = &rows[0];
    let rendered = row.render();
    let parsed = json::parse(&rendered).expect("the row parses");

    for path in [
        "labels.machine",
        "labels.runtime",
        "labels.date",
        "labels.engine_commit",
    ] {
        let value = reference::resolve(&parsed, path)
            .unwrap_or_else(|| panic!("the row carries no {path}: {rendered}"));
        let text = value
            .as_str()
            .unwrap_or_else(|| panic!("{path} is not one flat string: {rendered}"));
        assert!(!text.trim().is_empty(), "{path} is blank");
    }

    let label = |path: &str| {
        reference::resolve(&parsed, path)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    // The machine label names the box *and* the OS: a visitor comparing their Run
    // needs to know what the numbers came from, in one string.
    assert!(label("labels.machine").contains(&row.machine.cpu), "{}", label("labels.machine"));
    assert!(label("labels.machine").contains(&row.machine.os), "{}", label("labels.machine"));
    assert_eq!(label("labels.runtime"), row.machine.runtime);
    assert!(label("labels.runtime").contains("not a browser"), "a Node row says so");
    assert_eq!(label("labels.date"), row.measured_at);
    assert_eq!(label("labels.engine_commit"), row.engine.commit);
}

#[test]
fn the_row_id_names_the_commit_the_artifact_the_width_and_the_date() {
    let id = reference::run_id(&engine(), 25, "2026-11-03T02:30:00Z");
    assert_eq!(id, "0123456/3cb88e38/E1/w25/2026-11-03T02:30:00Z");
}

// ------------------------------------------------------- the append-only file

#[test]
fn reference_jsonl_is_append_only_and_never_rewrites_a_row() {
    let dir = temp_dir("append");
    let path = dir.join("reference.jsonl");
    fs::write(&path, "").expect("an empty log");

    let first = reference::rows(
        &[measurement(&[10_212, 9_810], &[8, 8])],
        &labels(0, None),
    )
    .remove(0);
    reference::append(&path, &first).expect("the first row");

    let before = fs::read(&path).expect("read");
    let (next, previous) = reference::next_index(&path).expect("the chain");
    assert_eq!(next, 1);
    assert_eq!(previous.as_deref(), Some(first.run_id.as_str()));

    let mut second_labels = labels(next, previous);
    second_labels.measured_at = "2026-11-04T09:00:00Z".to_string();
    let second = reference::rows(
        &[measurement(&[10_212, 9_811], &[8, 8])],
        &second_labels,
    )
    .remove(0);
    reference::append(&path, &second).expect("the second row");

    let after = fs::read(&path).expect("read");
    assert!(
        after.starts_with(&before),
        "an existing row was rewritten: {} -> {}",
        String::from_utf8_lossy(&before),
        String::from_utf8_lossy(&after)
    );
    assert_eq!(
        after.len(),
        before.len() + 1 + second.render().len(),
        "the file grew by exactly one line"
    );
    let text = String::from_utf8(after).expect("utf8");
    assert_eq!(text.lines().count(), 2);
    assert!(text.ends_with('\n'), "every row ends its own line");

    // A row whose index is not previous + 1 is refused: that is the mechanical
    // form of "a re-measurement adds an entry", not "a re-measurement replaces one".
    let error = reference::append(&path, &first).expect_err("a stale index is refused");
    assert!(error.contains("index"), "{error}");

    // The rows are in `measured_at` order, so `tail -1` is the row the page shows.
    let rows = reference::read(&path).expect("the rows read back");
    assert_eq!(rows.len(), 2);
    assert!(rows[0].measured_at <= rows[1].measured_at);
    assert_eq!(rows[1].supersedes.as_deref(), Some(first.run_id.as_str()));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn an_empty_log_starts_at_index_zero_with_nothing_to_supersede() {
    let dir = temp_dir("empty");
    let path = dir.join("reference.jsonl");
    fs::write(&path, "").expect("an empty log");
    assert_eq!(reference::next_index(&path).expect("empty"), (0, None));
    // An absent file is not an error either: the first Reference run creates it.
    let missing = dir.join("absent.jsonl");
    assert_eq!(reference::next_index(&missing).expect("absent"), (0, None));
    let _ = fs::remove_dir_all(&dir);
}

// -------------------------------------------------- the committed rows agree

/// §9.6: "Quality rows (win rate, lines, evaluations) are held **exact** for an
/// identical engine hash and Dataset fingerprint; machine rows may differ."
///
/// This is that sentence as a test over the committed file: every pair of rows with
/// one identity must carry identical quality rows, whatever their clocks say.
#[test]
fn quality_rows_are_exact_under_one_engine_hash() {
    let data = dataset::root();
    for config in CONFIGS {
        let path = dataset::reference_path(&data, &config);
        let rows = reference::read(&path).expect("the committed log reads");
        for (index, left) in rows.iter().enumerate() {
            for right in rows.iter().skip(index + 1) {
                if left.behaviour.width != right.behaviour.width
                    || left.behaviour.evaluator != right.behaviour.evaluator
                    || left.behaviour.toggles != right.behaviour.toggles
                    || left.engine.wasm_sha256 != right.engine.wasm_sha256
                    || left.dataset.fingerprint != right.dataset.fingerprint
                {
                    continue; // a different golden: the rows *should* differ
                }
                assert_eq!(
                    left.quality(),
                    right.quality(),
                    "{}: rows {} and {} share one identity but not one quality row — a quality \
                     row that moves under an identical engine hash is a behaviour change, not a \
                     re-measurement (§9.6)",
                    config.dir,
                    left.index,
                    right.index
                );
                assert_eq!(left.fingerprints, right.fingerprints);
            }
        }
    }
}

#[test]
fn a_committed_row_matches_the_manifest_it_was_measured_on() {
    let data = dataset::root();
    for config in CONFIGS {
        let manifest = read_manifest(&data, &config);
        let path = dataset::reference_path(&data, &config);
        for row in reference::read(&path).expect("the committed log reads") {
            assert_eq!(
                row.dataset.fingerprint, manifest.fingerprint,
                "{}: row {} was measured on a different Dataset",
                config.dir, row.index
            );
            assert_eq!(row.dataset.generator_version, manifest.generator_version);
            assert_eq!(row.behaviour.goal, config.goal);
            assert_eq!(row.behaviour.pieces, config.pieces);
            assert_eq!(row.behaviour.cap, config.cap);
        }
    }
}

fn read_manifest(data: &std::path::Path, config: &Config) -> dataset::Manifest {
    let path = dataset::manifest_path(data, config);
    let text = fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    dataset::Manifest::parse(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}
