//! Ticket 06: the width curve names `DEFAULT_BEAM` and the Width cap (§9.5).
//!
//! Two layers of test, deliberately:
//!
//! * the **rules** are pure functions with synthetic inputs — the exact McNemar
//!   test, the two-legged tie predicate, the nearest-rank p90, the cap and the
//!   default chosen by the pre-registered rule. These are the parts a re-run must
//!   not change, so they are pinned by arithmetic, not by a measurement.
//! * the **artifact** is exercised end to end through [`width::run`] at a
//!   deliberately tiny size (two doubling points, eight seeds), so the file the
//!   page reads is produced by the same code path that produced the committed one.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bench::width::{
    self, Artifact, Bed, BedCost, CapBound, CostRow, DefaultBasis, DefaultSource, MsSource,
    Options, Overshoot, Paired, Point, QualityAxis, QualityRow, Summary, Tally, WIDTHS,
};

const DEFAULT_BED: &str = "goal8-p40-cap7";
const HARDER_BED: &str = "goal12-p100-cap7";

// ------------------------------------------------------------------ fixtures

fn tal(wins: u32, games: u32) -> Tally {
    Tally { wins, games }
}

/// One quality row at the size of a real split: 1,000 paired games.
fn row(width: u32, wins: u32, b: u32, c: u32) -> QualityRow {
    QualityRow {
        width,
        tally: tal(wins, 1_000),
        paired: Paired { b, c },
    }
}

fn cost(width: u32, default_p90: f64, harder_p90: f64) -> CostRow {
    CostRow {
        width,
        p90_ms: [default_p90, harder_p90],
    }
}

fn scratch(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("tetris-lab-06-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// The smallest honest run: two doubling points, one trial of sixteen seeds, eight
/// quality games, native milliseconds. Milliseconds are the substrate the cap is
/// *not* named on (it is named on wasm), but every rule above them is the same,
/// which is what this run is testing.
///
/// Sixteen seeds, not eight: the nearest-rank p90 of eight samples *is* the
/// maximum, so one scheduler hiccup on a shared box would decide the cap and this
/// test would report a build failure that is not one.
///
/// Which width the cap lands on is not asserted anywhere below, deliberately: a
/// `cargo test` build is unoptimised and solves ~20× slower than the release build
/// the curve is measured in, so in debug the harder bed at width 50 already misses
/// the 200 ms budget and the cap is the narrowest point. The shape of the artifact
/// and the rules above the clock are the same either way, and those are the tests.
///
/// The probe is what makes the stop rule's verdict independent of the box: two of
/// nine pre-registered points are asked for, so the curve is *truncated*, and the
/// only honest way to say why the climb ended is the probe at width 100 — a budget
/// that failed, at a size no debug build's clock can disagree with.
fn quick(out: &Path) -> Options {
    Options {
        data: bench::dataset::root(),
        out: out.to_path_buf(),
        widths: vec![25, 50],
        trials: 1,
        trial_seeds: 16,
        quality_games: 8,
        ms_source: MsSource::Native,
        engine_commit: "test0sha".to_string(),
        machine: "a test box, not the reference box".to_string(),
        runtime: "native".to_string(),
        reason: "test run".to_string(),
        quiet: true,
        now_secs: 1_800_000_000,
        overshoot: Some(Overshoot {
            width: 100,
            substrate: "native".to_string(),
            samples: 3,
            ms_p90: 999.0,
            note: "a test probe, so the bound does not depend on this box's clock".to_string(),
        }),
    }
}

/// One measured run at a time. Four tests in this binary each time real solves, and
/// `cargo test` runs them in parallel: without this lock they charge each other's
/// scheduling to the p90 they are measuring.
static MEASURE: Mutex<()> = Mutex::new(());

fn run_locked(options: &Options) -> Artifact {
    let _guard = MEASURE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    width::run(options).unwrap_or_else(|error| panic!("run: {error}"))
}

fn measure_quick(tag: &str) -> Artifact {
    let dir = scratch(tag);
    let opts = quick(&dir.join("width-curve.json"));
    run_locked(&opts)
}

fn committed_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("data")
        .join("measurements")
        .join("width-curve.json")
}

fn bed_cost(bed: &str, ms_p90: f64) -> BedCost {
    BedCost {
        bed: bed.to_string(),
        matches: tal(600, 600),
        evaluations_p50: 1_000,
        ms_p50: ms_p90 * 0.8,
        ms_p90,
        ms_max: ms_p90 * 1.2,
        trials_ms_p90: vec![ms_p90],
    }
}

/// One point of a synthetic series: both shipped beds, at the milliseconds given.
fn measured(width: u32, default_p90: f64, harder_p90: f64) -> Point {
    Point {
        width,
        cost: vec![
            bed_cost(DEFAULT_BED, default_p90),
            bed_cost(HARDER_BED, harder_p90),
        ],
        quality: Some(QualityRow {
            width,
            tally: tal(1_000, 1_000),
            paired: Paired { b: 0, c: 0 },
        }),
        flat_step: None,
    }
}

/// The committed curve's own cost series, through width 800 — the six points the
/// run actually measured. Everything above 800 is a pre-registered point it did
/// not measure, which is what the summary fields have to be honest about.
fn measured_to_800() -> Vec<Point> {
    [
        (25, 4.421, 6.021),
        (50, 8.017, 10.865),
        (100, 14.977, 20.582),
        (200, 30.070, 40.087),
        (400, 57.254, 79.982),
        (800, 111.474, 157.357),
    ]
    .into_iter()
    .map(|(width, default_p90, harder_p90)| measured(width, default_p90, harder_p90))
    .collect()
}

/// The probe the committed artifact records: width 1,600 on a coarse native
/// sample, over the default configuration's 200 ms budget.
fn probe(width: u32, ms_p90: f64) -> Overshoot {
    Overshoot {
        width,
        substrate: "native".to_string(),
        samples: 10,
        ms_p90,
        note: "≈215 ms on the wasm substrate this curve's cost series is measured on"
            .to_string(),
    }
}

// ------------------------------------------------------- the pre-registered list

#[test]
fn the_doubling_points_are_the_ones_the_spec_pre_registered() {
    assert_eq!(WIDTHS, [25, 50, 100, 200, 400, 800, 1_600, 3_200, 6_400]);
}

#[test]
// ------------------------------------- the summary fields are derived, not requested

#[test]
fn a_truncated_width_list_cannot_produce_a_complete_curve_or_the_schedule() {
    // Six of §9.5's nine points, exactly as the committed run was invoked
    // (`--widths 25,…,800`), with nothing on the record to say why the climb
    // ended. The derivation refuses to name a bound: `schedule` is the word that
    // tells the page's tooltip "the curve was still climbing at the last measured
    // width", which is a lie about a curve that won 1000/1000 everywhere it was
    // measured. `complete: true` would be the same lie in a different field.
    let points = measured_to_800();
    let error = width::summarise(&points, None).expect_err("a truncated list is not a curve");
    assert!(error.contains("1600"), "the reason names the widths: {error}");
    assert!(error.contains("not measured"), "{error}");
}

#[test]
fn a_probe_that_fails_the_budget_is_what_makes_the_bound_a_budget() {
    let summary = width::summarise(&measured_to_800(), Some(&probe(1_600, 203.46)))
        .expect("a probe over the budget is evidence");
    assert_eq!(summary.width_cap, 800);
    assert_eq!(summary.cap_bound, CapBound::Budget);
    assert!(!summary.complete, "six of nine points were measured");
    assert_eq!(summary.missing_widths, vec![1_600, 3_200, 6_400]);
    // The one-line reason has to be checkable on its own: the width, the p90, the
    // substrate, the sample size, the fact that the probe is not a point, and why
    // the wider points are absent.
    for needle in [
        "1600",
        "203.46",
        "native",
        "10 seeds",
        "not as a measured point",
        "3200",
        "6400",
        "the sweep stops at the first width that fails a budget",
    ] {
        assert!(
            summary.incomplete_reason.contains(needle),
            "{needle:?} in {:?}",
            summary.incomplete_reason
        );
    }
    assert!(
        summary.incomplete_reason.contains("215 ms"),
        "the probe's own note travels with it: {:?}",
        summary.incomplete_reason
    );
}

#[test]
fn a_measured_failing_width_is_a_budget_too_and_the_point_stays_a_point() {
    // The other route to `budget`: the sweep measured the width above the cap and
    // it failed. No probe is needed, and the failing width is a point, not a probe.
    let mut points = measured_to_800();
    points.push(measured(1_600, 203.46, 290.0));
    let summary = width::summarise(&points, None).expect("a measured failure is evidence");
    assert_eq!(summary.width_cap, 800);
    assert_eq!(summary.cap_bound, CapBound::Budget);
    assert_eq!(summary.missing_widths, vec![3_200, 6_400]);
    assert!(summary.incomplete_reason.contains("1600"));
    assert!(summary.incomplete_reason.contains("203.46"));
}

#[test]
fn only_a_fully_measured_curve_can_name_the_schedule() {
    let all: Vec<Point> = WIDTHS
        .iter()
        .map(|width| measured(*width, *width as f64 / 100.0, *width as f64 / 50.0))
        .collect();
    let summary = width::summarise(&all, None).expect("every pre-registered point measured");
    assert_eq!(summary.cap_bound, CapBound::Schedule);
    assert!(summary.complete);
    assert!(summary.missing_widths.is_empty());
    assert_eq!(summary.incomplete_reason, "");
    assert_eq!(summary.width_cap, 6_400);

    // One point short — the widest, where a truncated request always stops — and
    // the word "schedule" is gone.
    assert!(width::summarise(&all[..8], None).is_err());
}

#[test]
fn a_probe_that_does_not_fail_the_budget_is_not_why_the_climb_stopped() {
    let points = measured_to_800();
    let under = probe(1_600, 199.9);
    assert!(
        width::summarise(&points, Some(&under)).is_err(),
        "a probe that fits the budget explains nothing"
    );
    let below_the_cap = probe(400, 900.0);
    assert!(
        width::summarise(&points, Some(&below_the_cap)).is_err(),
        "the cap fits a budget at 400, so a probe cannot fail one there"
    );
    let not_pre_registered = probe(1_000, 900.0);
    assert!(
        width::summarise(&points, Some(&not_pre_registered)).is_err(),
        "1000 is not one of §9.5's doubling points"
    );
    let mut no_samples = probe(1_600, 203.46);
    no_samples.samples = 0;
    assert!(
        width::summarise(&points, Some(&no_samples)).is_err(),
        "a p90 without a sample size is not a measurement"
    );
}

#[test]
fn a_run_that_was_asked_for_a_prefix_says_so_in_the_file() {
    // `quick` asks for two of the nine pre-registered points, which is the shape of
    // the committed run (`--widths 25,…,800`): a truncated list. The artifact must
    // not claim to be complete, and it must not claim the doubling list stopped it.
    let artifact = measure_quick("truncated");
    assert!(!artifact.complete);
    let expected: Vec<u32> = WIDTHS
        .iter()
        .copied()
        .filter(|width| !artifact.points.iter().any(|point| point.width == *width))
        .collect();
    assert!(expected.len() >= 7, "{expected:?}");
    assert_eq!(artifact.missing_widths, expected);
    assert_eq!(artifact.cap_bound, CapBound::Budget);
    assert!(
        artifact.incomplete_reason.contains("100"),
        "the probe at 100 is why the climb stopped: {:?}",
        artifact.incomplete_reason
    );

    let text = artifact.render();
    assert!(text.contains("\"complete\": false"));
    assert!(
        !text.contains("\"cap_bound\": \"schedule\""),
        "a truncated list cannot claim the schedule stopped it"
    );
}

#[test]
fn the_budgets_are_two_hundred_and_one_thousand_milliseconds() {
    // §9.3/§9.5: 200 ms is both the default configuration's budget and the cap's
    // criterion; the harder configuration gets 1 s.
    assert_eq!(width::DEFAULT_BUDGET_MS, 200);
    assert_eq!(width::HARDER_BUDGET_MS, 1_000);
}

// ------------------------------------------------------------ exact McNemar

#[test]
fn mcnemar_is_the_exact_two_sided_sign_test_on_the_discordant_pairs() {
    // p = 2 * sum_{k<=m} C(n,k) / 2^n, n = b + c, m = min(b, c).
    // No discordant pair is no evidence at all.
    assert!(!width::mcnemar_rejects(Paired { b: 0, c: 0 }));
    // b=0, c=10: p = 2/2^10 = 0.001953 — ten discordant all one way is 1% significant.
    assert!(width::mcnemar_rejects(Paired { b: 0, c: 10 }));
    // b=5, c=0: p = 2/2^5 = 0.0625 — a five-game hole is not.
    assert!(!width::mcnemar_rejects(Paired { b: 5, c: 0 }));
    // The 1% boundary: 2/2^7 = 0.015625 >= 0.01, 2/2^8 = 0.0078125 < 0.01.
    assert!(!width::mcnemar_rejects(Paired { b: 0, c: 7 }));
    assert!(width::mcnemar_rejects(Paired { b: 0, c: 8 }));
    assert!(!width::mcnemar_rejects(Paired { b: 7, c: 0 }));
    assert!(width::mcnemar_rejects(Paired { b: 8, c: 0 }));
    // The widest a 1,000-game pairing can be and still be significant: 84 games
    // of imbalance (458 vs 542, p = 0.00864).
    assert!(width::mcnemar_rejects(Paired { b: 458, c: 542 }));
    assert!(width::mcnemar_rejects(Paired { b: 542, c: 458 }));
}

#[test]
fn mcnemar_is_symmetric_in_its_two_arms() {
    for (b, c) in [(0, 9), (3, 17), (40, 60), (200, 260), (499, 501)] {
        assert_eq!(
            width::mcnemar_rejects(Paired { b, c }),
            width::mcnemar_rejects(Paired { b: c, c: b }),
            "b={b} c={c}"
        );
    }
}

// -------------------------------------------------------- the tie predicate

#[test]
fn a_tie_is_both_a_mcnemar_pass_and_a_one_point_shortfall() {
    let cap = tal(1_000, 1_000);
    // Identical: no discordant pair, no shortfall. The cap ties itself, which is
    // what makes "nothing ties" unreachable.
    assert!(width::tie(cap, cap, Paired { b: 0, c: 0 }));
    // One game behind: shortfall 0.1 pp, McNemar p = 1.
    assert!(width::tie(tal(999, 1_000), cap, Paired { b: 1, c: 0 }));
    // Exactly 1.0 pp behind with a discordance the exact test cannot resolve:
    // shortfall leg passes at 10 wins, p = 0.057.
    assert!(width::tie(tal(990, 1_000), cap, Paired { b: 40, c: 30 }));
    // 1.1 pp behind: 11 wins is over the pre-registered shortfall, even though
    // McNemar cannot reject at that discordance. This is the leg that rejects.
    assert!(!width::tie(tal(989, 1_000), cap, Paired { b: 40, c: 29 }));
    // 2.0 pp behind with balanced discordants: McNemar passes, the shortfall
    // rejects. §9.5's argument for needing two statistics.
    assert!(!width::mcnemar_rejects(Paired { b: 60, c: 40 }));
    assert!(!width::tie(tal(980, 1_000), cap, Paired { b: 60, c: 40 }));
    // 5 games behind, all one way: McNemar p = 0.0625, shortfall 0.5 pp -> tie.
    assert!(width::tie(tal(995, 1_000), cap, Paired { b: 5, c: 0 }));
    // 1 pp behind, all one way: p = 2/2^10 = 0.001953, so the McNemar leg rejects.
    assert!(!width::tie(tal(990, 1_000), cap, Paired { b: 10, c: 0 }));
}

#[test]
fn a_width_ahead_of_the_cap_is_never_behind_it() {
    let cap = tal(900, 1_000);
    // Judged 1,000, reference 900, 100 improvements and no regression: the
    // shortfall is negative (fine) and the discordance is decisive (not a tie).
    assert_eq!(width::shortfall_wins(tal(1_000, 1_000), cap), -100);
    assert!(!width::tie(tal(1_000, 1_000), cap, Paired { b: 0, c: 100 }));
    // One game ahead is not resolvable: still a tie.
    assert!(width::tie(tal(901, 1_000), cap, Paired { b: 0, c: 1 }));
}

#[test]
fn the_shortfall_leg_is_exact_integer_arithmetic() {
    assert_eq!(
        width::shortfall_wins(tal(990, 1_000), tal(1_000, 1_000)),
        10
    );
    assert_eq!(
        width::shortfall_wins(tal(1_000, 1_000), tal(990, 1_000)),
        -10
    );
    assert!(width::shortfall_within_one_point(
        tal(990, 1_000),
        tal(1_000, 1_000)
    ));
    assert!(!width::shortfall_within_one_point(
        tal(989, 1_000),
        tal(1_000, 1_000)
    ));
    // Unequal sample sizes must not fool it: 10 wins in 1,000 is 1.0 pp, but 10
    // wins in 2,000 is 0.5 pp.
    assert!(width::shortfall_within_one_point(
        tal(1_990, 2_000),
        tal(2_000, 2_000)
    ));
}

// ------------------------------------------------------------- the p90 rule

#[test]
fn p90_is_the_nearest_rank_order_statistic_and_is_not_interpolated() {
    let samples: Vec<f64> = (1..=1_000).map(|n| n as f64).collect();
    assert_eq!(width::p90(&samples), 900.0);
    assert_eq!(width::nearest_rank(&samples, 1, 2), 500.0);
    // Ten samples: the 9th, not 9.1.
    let ten: Vec<f64> = (1..=10).map(|n| n as f64).collect();
    assert_eq!(width::p90(&ten), 9.0);
    // Unsorted input is sorted, and the single-sample case is that sample.
    let unsorted = vec![3.0, 1.0, 2.0];
    assert_eq!(width::p90(&unsorted), 3.0);
    assert_eq!(width::p90(&[4.5]), 4.5);
}

// -------------------------------------------------------------- the Width cap

#[test]
fn the_cap_is_the_largest_measured_width_that_fits_both_budgets() {
    let rows = [
        cost(25, 6.0, 10.0),
        cost(50, 13.0, 21.0),
        cost(100, 27.0, 33.0),
        cost(200, 53.0, 66.0),
        cost(400, 240.0, 180.0), // the default configuration's budget binds
    ];
    assert_eq!(width::width_cap(&rows), Ok((200, CapBound::Budget)));
}

#[test]
fn the_harder_configuration_can_be_the_one_that_binds() {
    let rows = [cost(400, 100.0, 900.0), cost(800, 150.0, 1_400.0)];
    assert_eq!(width::width_cap(&rows), Ok((400, CapBound::Budget)));
}

#[test]
fn a_curve_that_never_fails_ends_at_the_last_measured_width() {
    let rows = [
        cost(25, 3.0, 5.0),
        cost(50, 6.0, 9.0),
        cost(100, 12.0, 20.0),
    ];
    assert_eq!(
        width::width_cap(&rows),
        Ok((100, CapBound::Schedule)),
        "the doubling list is what stopped it, not a budget"
    );
}

#[test]
fn a_curve_where_no_width_fits_is_an_error_not_a_zero() {
    let rows = [cost(25, 250.0, 1_500.0), cost(50, 500.0, 3_000.0)];
    assert!(width::width_cap(&rows).is_err());
    assert!(width::width_cap(&[]).is_err());
}

#[test]
fn an_unordered_curve_is_rejected_rather_than_silently_sorted() {
    let rows = [cost(100, 12.0, 20.0), cost(25, 3.0, 5.0)];
    assert!(width::width_cap(&rows).is_err());
}

// ------------------------------------------------------------ the default width

#[test]
fn the_default_is_the_narrowest_width_that_ties_the_cap() {
    let rows = [
        row(25, 970, 30, 0),   // 3 pp behind: no tie
        row(50, 1_000, 0, 0),  // ties
        row(100, 1_000, 0, 0), // ties, but wider
        row(200, 1_000, 0, 0), // the cap
    ];
    assert_eq!(
        width::default_beam(&rows),
        (50, DefaultSource::Tied),
        "ties resolve narrower"
    );
}

#[test]
fn the_default_may_equal_the_cap() {
    let rows = [
        row(25, 900, 100, 0),
        row(50, 950, 50, 0),
        row(400, 1_000, 0, 0),
    ];
    assert_eq!(
        width::default_beam(&rows),
        (400, DefaultSource::SelfTie),
        "the cap ties itself, so the rule always names something"
    );
}

#[test]
fn a_saturated_curve_names_the_narrowest_point_it_measured() {
    let rows = [
        row(25, 1_000, 0, 0),
        row(50, 1_000, 0, 0),
        row(100, 1_000, 0, 0),
        row(400, 1_000, 0, 0),
    ];
    assert_eq!(width::default_beam(&rows), (25, DefaultSource::Tied));
}

// --------------------------------------------------- the stop rule and the axis

#[test]
fn a_flat_step_is_the_tie_predicate_applied_to_adjacent_widths() {
    // No change at all: flat.
    assert!(width::flat(
        tal(1_000, 1_000),
        tal(1_000, 1_000),
        Paired { b: 0, c: 0 }
    ));
    // One game better, unresolvable: flat.
    assert!(width::flat(
        tal(1_000, 1_000),
        tal(999, 1_000),
        Paired { b: 0, c: 1 }
    ));
    // Still climbing: 100 seeds rescued from a 900-win point, decisive.
    assert!(!width::flat(
        tal(1_000, 1_000),
        tal(900, 1_000),
        Paired { b: 0, c: 100 }
    ));
    // Three games worse, all one way: McNemar p = 0.25, still a tie.
    assert!(width::flat(
        tal(997, 1_000),
        tal(1_000, 1_000),
        Paired { b: 3, c: 0 }
    ));
}

#[test]
fn the_sweep_stops_after_two_consecutive_flat_steps() {
    let climbing = (tal(900, 1_000), tal(1_000, 1_000), Paired { b: 0, c: 100 });
    let flat = (tal(1_000, 1_000), tal(1_000, 1_000), Paired { b: 0, c: 0 });
    assert_eq!(width::trailing_flat_steps(&[climbing, flat, flat]), 2);
    assert_eq!(width::trailing_flat_steps(&[flat]), 1);
    assert_eq!(width::trailing_flat_steps(&[climbing, climbing]), 0);
    assert!(width::sweep_is_flat(&[climbing, flat, flat]));
    assert!(!width::sweep_is_flat(&[climbing, flat]));
}

#[test]
fn the_quality_axis_says_so_when_the_ranking_bed_saturates() {
    // §9.1's threshold: >= 99% cannot rank anything, at any width.
    assert_eq!(
        width::quality_axis(tal(1_000, 1_000)),
        QualityAxis::NoResolution
    );
    assert_eq!(
        width::quality_axis(tal(990, 1_000)),
        QualityAxis::NoResolution
    );
    assert_eq!(width::quality_axis(tal(989, 1_000)), QualityAxis::Resolved);
}

// ------------------------------------------------------------- the timestamp

#[test]
fn the_timestamp_is_utc_and_parses_back() {
    assert_eq!(width::iso_utc(0), "1970-01-01T00:00:00Z");
    assert_eq!(width::iso_utc(1_800_000_000), "2027-01-15T08:00:00Z");
    // A leap day, which is where a hand-rolled civil-date conversion goes wrong.
    assert_eq!(width::iso_utc(1_709_164_800), "2024-02-29T00:00:00Z");
}

// ---------------------------------------------------------- the quality fingerprint

#[test]
fn the_quality_fingerprint_tracks_wins_and_ignores_milliseconds() {
    let point = |ms: f64| Point {
        width: 50,
        cost: vec![BedCost {
            bed: DEFAULT_BED.to_string(),
            matches: tal(1_000, 1_000),
            evaluations_p50: 19_052,
            ms_p50: ms,
            ms_p90: ms * 1.1,
            ms_max: ms * 1.4,
            trials_ms_p90: vec![ms * 1.1, ms * 1.05],
        }],
        quality: Some(row(50, 997, 3, 0)),
        flat_step: None,
    };
    // §9.6: quality rows are held exact for an identical engine hash and Dataset
    // fingerprint; machine rows may differ. Milliseconds must not move the hash.
    assert_eq!(
        width::quality_fingerprint(&[point(12.0)]),
        width::quality_fingerprint(&[point(99.5)])
    );
    let mut changed = point(12.0);
    changed.quality = Some(row(50, 996, 4, 0));
    assert_ne!(
        width::quality_fingerprint(&[point(12.0)]),
        width::quality_fingerprint(&[changed])
    );
    assert!(width::quality_fingerprint(&[point(12.0)]).starts_with("fnv1a64:"));
}

// ------------------------------------------------------------------ the artifact

#[test]
fn a_measured_curve_names_both_numbers_and_records_every_point() {
    let artifact = measure_quick("names-both");
    artifact
        .validate()
        .unwrap_or_else(|error| panic!("{error}"));

    assert!(WIDTHS.contains(&artifact.default_beam));
    assert!(WIDTHS.contains(&artifact.width_cap));
    assert!(artifact.default_beam >= 1);
    assert!(artifact.default_beam <= artifact.width_cap);

    // Both shipped beds are measured at every cost point, and the cap is the
    // largest passing width with its first failing neighbour recorded beside it.
    assert!(
        artifact.points.len() >= 2,
        "the curve records every point it measured"
    );
    for point in &artifact.points {
        assert_eq!(
            point.cost.len(),
            2,
            "both shipped beds at width {}",
            point.width
        );
        assert_eq!(point.cost[0].bed, DEFAULT_BED);
        assert_eq!(point.cost[1].bed, HARDER_BED);
        for bed in &point.cost {
            assert!(bed.ms_p90 > 0.0);
            assert!(bed.matches.wins <= bed.matches.games);
            // One trial of sixteen seeds, as `quick` asked for.
            assert_eq!(bed.matches.games, 16);
            assert_eq!(bed.trials_ms_p90.len(), 1);
        }
    }
    let ascending: Vec<u32> = artifact.points.iter().map(|point| point.width).collect();
    assert!(
        ascending.windows(2).all(|window| window[0] < window[1]),
        "points are recorded ascending: {ascending:?}"
    );
    assert!(ascending.contains(&artifact.width_cap));

    // §9.1's pilot ran first and is on the record, with the bed that ranked the
    // widths and why.
    assert_eq!(artifact.saturation.checked_at_width, artifact.width_cap);
    assert_eq!(artifact.saturation.beds.len(), 2);
    assert_eq!(artifact.saturation.quality_bed, Bed::GOAL16.name());
    assert!(!artifact.saturation.fallback_reason.is_empty());

    // The two identities a re-run must reproduce.
    assert_eq!(artifact.engine_commit, "test0sha");
    assert_eq!(artifact.datasets.len(), 2);
    for (name, fingerprint, _version) in &artifact.datasets {
        let manifest =
            fs::read_to_string(bench::dataset::root().join(name).join("manifest.json")).unwrap();
        assert!(
            manifest.contains(fingerprint),
            "{name}: the artifact's fingerprint is the committed one ({fingerprint})"
        );
    }
}

#[test]
fn the_artifact_is_written_by_the_bench_and_not_hand_edited() {
    let dir = scratch("written");
    let path = dir.join("width-curve.json");
    let artifact = run_locked(&quick(&path));
    let text = fs::read_to_string(&path).unwrap();

    assert!(text.ends_with("}\n"), "pretty-printed, newline at EOF");
    assert_eq!(
        width::json_i64(&text, "default_beam"),
        Some(artifact.default_beam as i64)
    );
    assert_eq!(
        width::json_i64(&text, "width_cap"),
        Some(artifact.width_cap as i64)
    );
    assert_eq!(width::json_i64(&text, "max_shortfall_tenths_pp"), Some(10));
    // §9.3: the retired 50 ms figure must not reappear anywhere.
    assert!(!text.contains("50 ms"), "the 50 ms budget is retired");
    assert!(!text.to_lowercase().contains("fifty"));
    // The two budgets, as numbers, on the record.
    assert!(text.contains("\"default_ms\": 200"));
    assert!(text.contains("\"harder_ms\": 1000"));
    // The curve's own honesty about its coverage, and the stop rule's verdict. Two
    // of nine pre-registered points were asked for, so the artifact says which seven
    // are missing, why the climb stopped (the probe), and that it did *not* run out
    // of schedule — the two fields the committed defect got wrong.
    assert!(
        text.contains("\"complete\": false"),
        "two of nine points were measured"
    );
    assert!(
        text.contains("\"cap_bound\": \"budget\""),
        "a probed failure ended the sweep"
    );
    assert!(
        !text.contains("\"cap_bound\": \"schedule\""),
        "a truncated list cannot claim the schedule stopped it"
    );
    assert!(!text.contains("\"missing_widths\": []"));
    assert!(!text.contains("\"incomplete_reason\": \"\""));
    assert!(text.contains("\"overshoot\": {"));
    assert!(text.contains("\"probe\": true"));
    assert!(text.contains("\"quality_stop_rule_fired_at\""));
}

#[test]
fn a_curve_that_did_not_measure_every_point_says_which_ones_and_why() {
    // The default width list is the pre-registered nine; the cost sweep stops at
    // the first failure, so the wider points are unmeasured by rule, not by
    // accident. That is `complete: false` with a reason — not a curve that claims
    // to have measured 6,400.
    let mut artifact = measure_quick("coverage");
    artifact.complete = false;
    artifact.missing_widths = vec![3_200, 6_400];
    assert!(
        artifact.validate().is_err(),
        "a partial curve must say why the widths are missing"
    );
    artifact.incomplete_reason =
        "the cost sweep stopped at width 1600 (the pre-registered stop rule)".to_string();
    assert_eq!(artifact.validate(), Ok(()));

    // And the other direction: a complete curve cannot carry either field.
    artifact.complete = true;
    artifact.missing_widths = vec![3_200, 6_400];
    assert!(
        artifact.validate().is_err(),
        "a complete curve measured every doubling point, so nothing can be missing"
    );
    artifact.missing_widths.clear();
    assert!(
        artifact.validate().is_err(),
        "a complete curve has nothing to explain"
    );
    artifact.incomplete_reason.clear();
    assert!(
        artifact.validate().is_err(),
        "a complete curve measured every point, so a probe above the cap is a contradiction"
    );
    // A "budget" bound is a claim about evidence: with the probe gone and no
    // measured failing width above the cap, it cannot stand either.
    artifact.overshoot = None;
    assert!(artifact.validate().is_err(), "budget with no evidence");
    // Only now is it a whole schedule that ran out, which is the one state that may
    // say so.
    artifact.cap_bound = CapBound::Schedule;
    assert_eq!(artifact.validate(), Ok(()));
}

#[test]
fn a_partial_curve_may_not_claim_the_schedule_stopped_it() {
    // The committed defect, in one field: six of nine points, `cap_bound:
    // "schedule"`. The tooltip built on that word says the curve was still climbing
    // at the last measured width, so the artifact refuses to say it.
    let mut artifact = measure_quick("schedule");
    assert!(!artifact.complete, "two of nine points were measured");
    artifact.cap_bound = CapBound::Schedule;
    let error = artifact.validate().expect_err("a truncated curve is not a schedule");
    assert!(error.contains("schedule"), "{error}");
}

#[test]
fn validate_refuses_a_curve_whose_quality_axis_is_still_pending() {
    // The in-flight copy a run rewrites mid-measurement is not a result: it must
    // not be mistakable for one.
    let mut artifact = measure_quick("pending");
    artifact.quality_axis = QualityAxis::Pending;
    assert!(artifact.validate().is_err());
}

#[test]
fn validate_refuses_a_width_below_the_cap_with_no_wins_recorded() {
    let mut artifact = measure_quick("no-quality-row");
    // §9.5's tie is judged against the cap, so the cap's own row is a row: a curve
    // that names a default from wins it did not record is not evidence.
    let cap = artifact.width_cap;
    let point = artifact
        .points
        .iter_mut()
        .find(|point| point.width == cap)
        .unwrap_or_else(|| panic!("the cap {cap} is a measured point"));
    assert!(point.quality.is_some(), "the cap carries its wins");
    point.quality = None;
    assert!(artifact.validate().is_err());
}

#[test]
fn a_saturated_pilot_is_recorded_as_having_no_quality_resolution() {
    let artifact = measure_quick("axis");
    // Eight games a point cannot distinguish anything, so the axis has no
    // resolution and the artifact must say so rather than dress a cost decision
    // up as a quality result.
    assert_eq!(artifact.quality_axis, QualityAxis::NoResolution);
    assert_eq!(artifact.default_beam_basis, DefaultBasis::Cost);
    assert_eq!(artifact.default_beam, 25);
    // And the one line a reader reads first says it plainly, with the counts that
    // justify the fallback.
    assert!(
        artifact.reason.contains("quality axis could not separate"),
        "reason: {}",
        artifact.reason
    );
    assert!(
        artifact.reason.contains("named on cost"),
        "{}",
        artifact.reason
    );
    assert!(
        artifact.reason.contains("goal16-p160-cap7 wins 8/8"),
        "{}",
        artifact.reason
    );
}

#[test]
fn validate_refuses_an_artifact_the_page_could_not_use() {
    let mut artifact = measure_quick("validate-cap");
    artifact.width_cap = 3;
    assert!(
        artifact.validate().is_err(),
        "the cap is a measured doubling point"
    );

    let mut artifact = measure_quick("validate-default");
    artifact.default_beam = artifact.width_cap + 1;
    assert!(
        artifact.validate().is_err(),
        "the default cannot exceed the cap"
    );

    let mut artifact = measure_quick("validate-empty");
    artifact.points.clear();
    assert!(
        artifact.validate().is_err(),
        "an artifact with no points is not a curve"
    );
}

// -------------------------------------------------- the committed artifact

#[test]
fn the_committed_artifact_names_a_default_and_a_cap() {
    let path = committed_path();
    let text = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "read {}: {error} — run `cargo run -p bench --release -- width-curve`",
            path.display()
        )
    });
    let default_beam = width::json_i64(&text, "default_beam").expect("default_beam");
    let width_cap = width::json_i64(&text, "width_cap").expect("width_cap");
    assert!(default_beam >= 1);
    assert!(default_beam <= width_cap);
    assert!(WIDTHS.iter().any(|width| *width as i64 == width_cap));
    assert!(WIDTHS.iter().any(|width| *width as i64 == default_beam));

    // The page's own module reads this file, and only this file.
    let page = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("web")
            .join("src")
            .join("config")
            .join("width.ts"),
    )
    .expect("web/src/config/width.ts");
    assert!(page.contains("data/measurements/width-curve.json"));
    // Node exposes a JSON module through `default` only; Vite would also accept
    // named re-exports. The module therefore projects the artifact's two fields
    // onto the two exported constants — and names nothing else, so the numbers
    // cannot be written a second time here.
    assert!(page.contains("curve.default_beam"));
    assert!(page.contains("export const DEFAULT_BEAM"));
    assert!(page.contains("curve.width_cap"));
    assert!(page.contains("export const WIDTH_CAP"));
}

#[test]
fn the_committed_artifact_names_the_stop_rule_and_its_own_coverage() {
    // The certificate a reader checks without re-running anything: what stopped the
    // sweep, what it did not measure, and the probe that is evidence but not a point.
    let text = fs::read_to_string(committed_path()).expect("the committed width curve");
    let artifact = Artifact::from_text(&text).expect("the committed artifact parses back");
    assert_eq!(artifact.width_cap, 800);
    assert_eq!(artifact.default_beam, 25);
    assert_eq!(artifact.cap_bound, CapBound::Budget);
    assert!(!artifact.complete, "six of nine pre-registered points");
    assert_eq!(artifact.missing_widths, vec![1_600, 3_200, 6_400]);
    assert!(
        artifact.incomplete_reason.contains("203.46"),
        "{:?}",
        artifact.incomplete_reason
    );

    let probe = artifact
        .overshoot
        .as_ref()
        .expect("the probe is on the record, not in prose");
    assert_eq!(probe.width, 1_600);
    assert!((probe.ms_p90 - 203.46).abs() < 0.005, "{}", probe.ms_p90);
    assert_eq!(probe.substrate, "native");
    assert_eq!(probe.samples, 10);
    assert!(
        !artifact
            .points
            .iter()
            .any(|point| point.width == probe.width),
        "a probe is not a measured point"
    );
    // And the wider points are absent from the series, not zero-filled.
    for width in [1_600, 3_200, 6_400] {
        assert!(!artifact.points.iter().any(|point| point.width == width));
    }
}

#[test]
fn the_committed_artifact_round_trips_through_the_reader_and_the_writer() {
    // The reader exists so `--summarise` can rewrite one artifact in place, so it
    // has to reproduce the file exactly: a float that came back with a different
    // number of decimals would be a measured number silently changing.
    let text = fs::read_to_string(committed_path()).expect("the committed width curve");
    let artifact = Artifact::from_text(&text).expect("parse");
    assert_eq!(
        artifact.render(),
        text,
        "the reader and the writer agree byte for byte"
    );
}

#[test]
fn summarising_the_committed_artifact_rewrites_only_the_derived_fields() {
    // §The ticket's own constraint: the committed cost and quality series survive
    // byte for byte. This is the command that says so, run on a copy.
    let dir = scratch("summarise");
    let path = dir.join("width-curve.json");
    let committed = fs::read_to_string(committed_path()).expect("the committed width curve");
    fs::write(&path, &committed).unwrap();

    let report = width::summarise_file(&path, &path, None).expect("summarise");
    assert_eq!(
        report.before, report.after,
        "re-deriving from the evidence already in the file changes nothing"
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        committed,
        "and the file is untouched"
    );
}

#[test]
fn a_probe_turns_the_bound_into_a_budget_without_moving_a_measured_number() {
    // The regeneration this ticket exists for, in miniature: an artifact that
    // claims the schedule ran out, plus the probe that shows a budget ended it.
    let dir = scratch("probe");
    let path = dir.join("width-curve.json");
    let committed = fs::read_to_string(committed_path()).expect("the committed width curve");

    // The artifact as it was committed when the fields were derived from the
    // requested list: the measured series is intact, the four summary fields lie.
    let mut stale = Artifact::from_text(&committed).expect("parse");
    stale.cap_bound = CapBound::Schedule;
    stale.complete = true;
    stale.missing_widths.clear();
    stale.incomplete_reason.clear();
    stale.overshoot = None;
    fs::write(&path, stale.render()).unwrap();

    let report = width::summarise_file(&path, &path, Some(probe(1_600, 203.46)))
        .expect("the probe explains the stop");
    assert_eq!(report.after.cap_bound, CapBound::Budget);
    assert!(!report.after.complete);
    assert_eq!(report.after.missing_widths, vec![1_600, 3_200, 6_400]);
    assert!(report.after.incomplete_reason.contains("203.46"));

    // Every measured point, both numbers and the quality fingerprint are the ones
    // that were committed: the fix is to derived fields, not to the measurement.
    assert_eq!(report.after.points, stale.points);
    assert_eq!(report.after.width_cap, stale.width_cap);
    assert_eq!(report.after.default_beam, stale.default_beam);
    assert_eq!(report.after.quality_fingerprint, stale.quality_fingerprint);
    let rewritten = fs::read_to_string(&path).unwrap();
    assert!(
        rewritten.contains("\"ms_p90\": 111.474"),
        "width 800's default-bed p90 is the measured one"
    );
}

#[test]
fn summarise_refuses_a_file_whose_measured_numbers_would_move() {
    // A hand-edited p90: the writer cannot keep four decimals, so re-rendering
    // would change a measured number. The command refuses instead of quietly
    // rounding somebody's evidence.
    let dir = scratch("hand-edited");
    let path = dir.join("width-curve.json");
    let committed = fs::read_to_string(committed_path()).expect("the committed width curve");
    let edited = committed.replace("\"ms_p90\": 111.474", "\"ms_p90\": 111.4744");
    assert_ne!(edited, committed, "the edit landed");
    fs::write(&path, &edited).unwrap();

    let error = width::summarise_file(&path, &path, None).expect_err("refuse to move a p90");
    assert!(error.contains("measured"), "{error}");
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        edited,
        "and the file is left exactly as it was found"
    );
}
