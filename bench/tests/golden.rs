//! Ticket 09's golden fingerprint: the exact compare, the identities it refuses to
//! guess about, and the 4x wall-clock bound that is not allowed to police behaviour.
//!
//! The compare is exercised against the committed golden itself — the expected side
//! is real data, and the measured side is built from it, so a mismatch is a real
//! seed with a real expected/actual pair rather than a made-up number.

use std::path::PathBuf;

use bench::dataset::{self, Config, CONFIGS, GOAL12};
use bench::gate::{self, State};
use bench::golden::{self, Golden, Identity};
use bench::reference::{self, DatasetId, Measurement};
use bench::width;

fn golden_path() -> PathBuf {
    let data = dataset::root();
    let measurements = data
        .parent()
        .expect("data/dataset has a parent: data/")
        .join("measurements");
    measurements.join(gate::GOLDEN_FILE)
}

fn committed() -> Golden {
    golden::load(&golden_path()).expect("the golden is committed and well formed")
}

/// A measurement that reproduces the committed golden exactly: the same per-seed
/// vectors, the same clock. The expected side of every assertion below is real.
fn measured_from_golden(golden: &Golden, config: Config) -> Measurement {
    let entry = golden
        .entry(config.dir)
        .unwrap_or_else(|| panic!("the golden carries {}", config.dir));
    Measurement::from_vectors(
        config,
        golden.behaviour.width,
        golden.split,
        &entry.evals,
        &entry.lines,
        entry.stats.batch_ms,
        entry.stats.cal_ms,
    )
}

fn identity_from_golden(golden: &Golden) -> Identity {
    golden.identity()
}

fn checks_for(golden: &Golden, identity: &Identity, measurements: &[Measurement]) -> Vec<bench::gate::Check> {
    golden::checks(golden, identity, measurements)
}

fn named<'a>(checks: &'a [bench::gate::Check], needle: &str) -> &'a bench::gate::Check {
    checks
        .iter()
        .find(|check| check.name.contains(needle))
        .unwrap_or_else(|| {
            panic!(
                "no check named {needle:?}; have {:?}",
                checks.iter().map(|check| check.name.as_str()).collect::<Vec<_>>()
            )
        })
}

fn text(check: &bench::gate::Check) -> String {
    format!("{}\n{}\n{:?}", check.subject, check.lines.join("\n"), check.hint)
}

// ------------------------------------------------------- the committed file

#[test]
fn the_committed_golden_names_the_engine_the_dataset_and_the_width() {
    let golden = committed();

    // The engine hash is 64 lowercase hex digits: the artifact the page executes.
    assert_eq!(golden.engine.wasm_sha256.len(), 64, "{}", golden.engine.wasm_sha256);
    assert!(golden
        .engine
        .wasm_sha256
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    assert!(golden.engine.wasm_bytes > 0);
    assert!(golden.engine.profile.contains("lto=fat"), "{}", golden.engine.profile);
    assert!(!golden.engine.commit.is_empty());

    // The Dataset identity is the committed manifest's, both configurations.
    let data = dataset::root();
    for config in CONFIGS {
        let path = dataset::manifest_path(&data, &config);
        let text = std::fs::read_to_string(&path).expect("the manifest reads");
        let manifest = dataset::Manifest::parse(&text).expect("the manifest parses");
        let recorded = golden
            .dataset(config.dir)
            .unwrap_or_else(|| panic!("the golden names {}", config.dir));
        assert_eq!(recorded.fingerprint, manifest.fingerprint, "{}", config.dir);
        assert_eq!(recorded.generator_version, manifest.generator_version);
    }

    // `DEFAULT_BEAM` is a golden number (§9.5): the golden's width is the committed
    // curve's, not a second opinion.
    let curve = width::Artifact::from_text(
        &std::fs::read_to_string(
            dataset::root()
                .parent()
                .expect("data/")
                .join("measurements")
                .join("width-curve.json"),
        )
        .expect("the width curve reads"),
    )
    .expect("the width curve parses");
    assert_eq!(golden.behaviour.width, curve.default_beam);
    assert_eq!(golden.behaviour.width_source, "artifact");

    // §9.4's ladder is the authority for what ships (ticket 08), so the golden's
    // evaluator labels are checked against it rather than against a constant alone.
    let ladder: bench::json::Value = bench::json::parse(
        &std::fs::read_to_string(
            dataset::root()
                .parent()
                .expect("data/")
                .join("measurements")
                .join("ablation-ladder.json"),
        )
        .expect("the ladder reads"),
    )
    .expect("the ladder parses");
    let shipped = ladder.get("shipped").expect("the ladder names what ships");
    assert_eq!(
        shipped.get("evaluator").and_then(|value| value.as_str()),
        Some("e1")
    );
    assert_eq!(
        shipped.get("table").and_then(|value| value.as_str()),
        Some(reference::EVALUATOR_TABLE)
    );

    // The split is §9.6's: the test Split, all of it.
    assert_eq!(golden.split.first, 0);
    assert_eq!(golden.split.last, 999);
    assert_eq!(golden.split.n(), 1000);
}

#[test]
fn the_committed_golden_is_the_committed_reference_rows() {
    // §9.6's rule is that the golden is pinned *from* a committed row, never from an
    // unrecorded run: the digests, the quality rows and the run id all have to be
    // the row's. The log is append-only, so a later row is *not* a candidate — the
    // golden names the row it was pinned from, and that row has to still be there.
    let golden = committed();
    let data = dataset::root();
    for config in CONFIGS {
        let path = dataset::reference_path(&data, &config);
        let rows = reference::read(&path).expect("the log reads");
        let named = golden
            .reference_run(config.dir)
            .unwrap_or_else(|| panic!("{}: the golden names no run", config.dir));
        let row = rows
            .iter()
            .find(|row| row.run_id == named)
            .unwrap_or_else(|| {
                panic!(
                    "{}: the golden was pinned from {named}, which is not in {} (rows: {:?})",
                    config.dir,
                    path.display(),
                    rows.iter().map(|row| row.run_id.as_str()).collect::<Vec<_>>()
                )
            });
        // The row it names is the row it was pinned on: the same bed, the same games,
        // the same width. A golden re-pointed at another row without re-pinning the
        // numbers would fail here rather than at the first moved seed.
        assert_eq!(
            Some(&row.dataset.fingerprint),
            golden.dataset(config.dir).map(|id| &id.fingerprint),
            "{}: the row's Dataset is the golden's",
            config.dir
        );
        assert_eq!(row.behaviour.width, golden.behaviour.width, "{}", config.dir);
        assert_eq!(row.engine.wasm_sha256, golden.engine.wasm_sha256, "{}", config.dir);
        let entry = golden.entry(config.dir).expect("an entry");

        assert_eq!(entry.evals_fnv1a64, row.fingerprints.evals_fnv1a64, "{}", config.dir);
        assert_eq!(entry.lines_fnv1a64, row.fingerprints.lines_fnv1a64, "{}", config.dir);
        assert_eq!(entry.stats.wins, row.stats.wins);
        assert_eq!(entry.stats.n_fail, row.stats.n_fail);
        assert_eq!(entry.stats.fail_lines_mean, row.stats.fail_lines_mean);
        assert_eq!(entry.stats.fail_lines_median, row.stats.fail_lines_median);
        assert_eq!(entry.stats.evals_p50, row.stats.evals_p50);
        assert_eq!(entry.stats.evals_p90, row.stats.evals_p90);
    }
}

// --------------------------------------------------------- the exact compare

#[test]
fn an_identical_measurement_passes_every_check() {
    let golden = committed();
    let measurements: Vec<Measurement> = CONFIGS
        .iter()
        .map(|config| measured_from_golden(&golden, *config))
        .collect();
    let checks = checks_for(&golden, &identity_from_golden(&golden), &measurements);

    for check in &checks {
        assert_eq!(
            check.state,
            State::Pass,
            "{}: {}",
            check.name,
            text(check)
        );
    }
    let evals = named(&checks, "golden evaluations");
    assert!(evals.subject.contains("1000 of 1000 seeds"), "{}", evals.subject);
    assert!(
        evals.subject.contains(golden.entry("goal8-p40-cap7").unwrap().evals_fnv1a64.as_str()),
        "{}",
        evals.subject
    );
}

#[test]
fn a_quality_row_that_moves_under_an_identical_hash_fails_and_names_the_seed() {
    // The engine now returns 3 more Evaluations for seed 37 — one row of one bed,
    // and nothing else. The engine hash and the Dataset fingerprint are the committed
    // ones, so this is the shape of a real behaviour change: the measured side moved
    // while the golden did not, and no re-measurement may absorb it.
    let golden = committed();
    let seed = 37usize;
    let expected = golden.entry("goal8-p40-cap7").expect("an entry").evals[seed];
    let actual = expected + 3;

    let measurements: Vec<Measurement> = CONFIGS
        .iter()
        .map(|config| {
            let mut measurement = measured_from_golden(&golden, *config);
            if config.dir == "goal8-p40-cap7" {
                measurement.samples[seed].evaluations = actual;
            }
            measurement
        })
        .collect();
    let checks = checks_for(&golden, &identity_from_golden(&golden), &measurements);

    let evals = named(&checks, "golden evaluations · goal8-p40-cap7");
    assert_eq!(evals.state, State::Fail, "{}", text(evals));
    let body = text(evals);
    assert!(body.contains("seed 37"), "{body}");
    assert!(body.contains(&expected.to_string()), "{body}");
    assert!(body.contains(&actual.to_string()), "{body}");
    assert!(body.contains("999 of 1000 seeds match"), "{body}");
    // The classification is what keeps a slow box from printing this text.
    let hint = evals.hint.as_deref().expect("a classification");
    assert!(hint.contains("BEHAVIOUR"), "{hint}");
    assert!(!hint.contains("MACHINE"), "{hint}");

    // The other configuration is untouched and still passes.
    assert_eq!(
        named(&checks, "golden evaluations · goal12-p100-cap7").state,
        State::Pass
    );
}

#[test]
fn the_golden_says_which_identity_changed_and_refuses_to_compare_anyway() {
    let golden = committed();
    let measurements: Vec<Measurement> = CONFIGS
        .iter()
        .map(|config| measured_from_golden(&golden, *config))
        .collect();

    // 1. The Dataset moved: the games are not the games the golden was pinned on.
    let mut identity = identity_from_golden(&golden);
    for (name, id) in identity.datasets.iter_mut() {
        if name == "goal8-p40-cap7" {
            *id = DatasetId {
                fingerprint: "0000000000000000".to_string(),
                generator_version: id.generator_version + 1,
            };
        }
    }
    let checks = checks_for(&golden, &identity, &measurements);
    let ident = named(&checks, "golden identity");
    assert_eq!(ident.state, State::Fail, "{}", text(ident));
    let body = text(ident);
    assert!(body.contains("dataset fingerprint"), "{body}");
    assert!(body.contains("4bf1a17b1685a774"), "{body}");
    assert!(body.contains("0000000000000000"), "{body}");
    assert!(!body.contains("wasm_sha256"), "only the dataset moved: {body}");
    // Nothing is compared against a golden whose games are different: there is no
    // line here that can pass or fail on behaviour.
    assert!(
        !checks.iter().any(|check| check.name.contains("golden evaluations")),
        "a different Dataset must not be reported as a behaviour change"
    );

    // 2. The engine moved: same games, a different artifact.
    let mut identity = identity_from_golden(&golden);
    identity.engine.wasm_sha256 = "00".repeat(32);
    let checks = checks_for(&golden, &identity, &measurements);
    let ident = named(&checks, "golden identity");
    assert_eq!(ident.state, State::Fail, "{}", text(ident));
    let body = text(ident);
    assert!(body.contains("engine"), "{body}");
    assert!(
        body.contains(&"00".repeat(32)) || body.contains("wasm sha256"),
        "{body}"
    );
}

#[test]
fn the_four_x_bound_is_normalised_by_the_calibration_and_never_touches_quality() {
    // A box that is uniformly 3.9x slower: identical quality rows, and the
    // normalised ratio is 1.0, so this passes — a slow machine is not a red gate.
    let golden = committed();
    let entry = golden.entry("goal8-p40-cap7").expect("an entry");

    let bound = golden::bound(entry.stats.batch_ms, entry.stats.cal_ms, entry.stats.batch_ms * 3.9, entry.stats.cal_ms * 3.9);
    assert!(bound.passes(), "{:?}", bound);
    assert!((bound.normalised - 1.0).abs() < 1e-9, "{:?}", bound);
    assert!(bound.raw > 3.8, "{:?}", bound);

    // A box whose clock is uniformly slow but whose *normalised* time is 5x: the
    // same work now costs 5x the per-unit time. That is the wall-clock assertion.
    let bound = golden::bound(
        entry.stats.batch_ms,
        entry.stats.cal_ms,
        entry.stats.batch_ms * 5.0,
        entry.stats.cal_ms,
    );
    assert!(!bound.passes(), "{:?}", bound);
    assert!((bound.normalised - 5.0).abs() < 1e-9, "{:?}", bound);
    assert_eq!(golden::WALL_CLOCK_FACTOR, 4.0);
}

#[test]
fn a_slow_box_trips_only_the_wall_clock_bound() {
    let golden = committed();
    let mut measurements: Vec<Measurement> = CONFIGS
        .iter()
        .map(|config| measured_from_golden(&golden, *config))
        .collect();
    // Identical integers; the clock is five times slower than the labelled box's.
    measurements[0].batch_ms *= 5.0;

    let checks = checks_for(&golden, &identity_from_golden(&golden), &measurements);
    assert_eq!(
        named(&checks, "golden evaluations · goal8-p40-cap7").state,
        State::Pass,
        "a slow box cannot move an Evaluation count"
    );
    assert_eq!(
        named(&checks, "golden lines · goal8-p40-cap7").state,
        State::Pass
    );

    let clock = named(&checks, "golden wall clock · goal8-p40-cap7");
    assert_eq!(clock.state, State::Fail, "{}", text(clock));
    let body = text(clock);
    assert!(body.contains("5.00x"), "{body}");
    let hint = clock.hint.as_deref().expect("a classification");
    assert!(hint.contains("MACHINE"), "{hint}");
    assert!(hint.contains("not a behaviour change"), "{hint}");
    assert!(!hint.contains("BEHAVIOUR CHANGE"), "{hint}");

    // The second configuration's clock was not touched.
    assert_eq!(
        named(&checks, "golden wall clock · goal12-p100-cap7").state,
        State::Pass
    );
}

#[test]
fn a_slow_but_proportionally_slow_box_only_warns() {
    // Raw batch 8x, calibration 8x too: the normalised ratio is 1.0. The gate passes
    // and says the raw ratio out loud, because a reader comparing two machines wants
    // the number even when the gate is green.
    let golden = committed();
    let mut measurements: Vec<Measurement> = CONFIGS
        .iter()
        .map(|config| measured_from_golden(&golden, *config))
        .collect();
    measurements[0].batch_ms *= 8.0;
    measurements[0].cal_ms_before *= 8.0;
    measurements[0].cal_ms_after *= 8.0;

    let checks = checks_for(&golden, &identity_from_golden(&golden), &measurements);
    let clock = named(&checks, "golden wall clock · goal8-p40-cap7");
    assert_eq!(clock.state, State::Pass, "{}", text(clock));
    assert!(clock.warn, "a raw ratio over the bound is worth looking at");
    assert!(text(clock).contains("8.00x"), "{}", text(clock));
}

#[test]
fn the_golden_is_a_small_enough_file_to_read_in_a_diff() {
    // One number per line, seed order: the line offset *is* the seed, which is what
    // makes "which seed moved" a question the file can answer without a tool.
    let golden = committed();
    let entry = golden.entry("goal8-p40-cap7").expect("an entry");
    let rendered = golden::render(&golden);
    assert!(rendered.ends_with('\n'));
    assert!(
        rendered.contains("      \"evals\": [\n"),
        "one number per line: {}",
        rendered.lines().take(40).collect::<Vec<_>>().join("\n")
    );
    assert_eq!(entry.evals.len(), 1000);
    assert_eq!(entry.lines.len(), 1000);

    // Round trip: the renderer's own output is what `load` accepts, byte for byte.
    let again = golden::from_text(&rendered).expect("render then parse");
    assert_eq!(again.entry("goal8-p40-cap7").unwrap().evals, entry.evals);
    assert_eq!(golden::render(&again), rendered);
}

#[test]
fn the_median_lines_row_is_null_not_zero_on_a_saturated_bed() {
    // GOAL12 is saturated at DEFAULT_BEAM; that is a fact about the bed, not a
    // formatting choice, and the golden has to carry it as `null`.
    let golden = committed();
    let entry = golden.entry(GOAL12.dir).expect("an entry");
    assert_eq!(entry.stats.n_fail, 0);
    assert_eq!(entry.stats.fail_lines_mean, None);
    assert_eq!(entry.stats.fail_lines_median, None);
    assert!(golden::render(&golden).contains("\"fail_lines_mean\": null"));
}
