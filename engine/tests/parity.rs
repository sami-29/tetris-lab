//! Native/wasm score parity: one committed Board-and-Sequence set, solved by
//! `cargo test` here and by the built wasm in `web/test/score-parity.test.mjs`.
//!
//! The ABI carries no score: §4's exports return a packed result and a Move list,
//! and the trace that does carry scores is `solve_traced` (ticket 11). So parity
//! is proven through `solve` — the packed `i64` **and** every word of the Move
//! list, which is the beam's layer-by-layer ranking played out. That is why the
//! set carries a Beam width of 1 as well as wide beams: at width 1 the answer is
//! the arg-max of the scores at every Layer, so a score that differs by one unit
//! anywhere on the winning path changes the Move list.
//!
//! The report also carries E0's result for every case, which the wasm cannot
//! reach (it can only call `shipped()`), for one reason: a set whose two tables
//! agreed everywhere would compare Move lists without ever observing a score.
//! `the_parity_set_can_tell_the_two_tables_apart` asserts that it does observe
//! them, and the "shipped" marker routes this suite and the node suite to the same
//! rows, so §9.4's verdict moves both or neither.

use std::path::{Path, PathBuf};

use engine::evaluator::{
    shipped, DellacherieThiery, Evaluator, Heuristic, E0_HEURISTIC, E1_PUBLISHED,
};
use engine::generator::generate_game;
use engine::search::{solve, SolveConfig};

/// The table label of one report row.
const TABLES: [&str; 2] = ["e0", "e1"];

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn table(label: &str) -> &'static dyn Evaluator {
    match label {
        "e0" => &Heuristic(E0_HEURISTIC),
        "e1" => &DellacherieThiery(E1_PUBLISHED),
        other => panic!("`{other}` is not a committed weight table"),
    }
}

/// One case of the set: a Seed, a piece count and a height cap — which is a Board
/// and a Sequence, by §5.5 — and the Goal and Beam its targets solve at.
#[derive(Clone, Copy, Debug)]
struct Case {
    seed: u32,
    pieces: u32,
    cap: u32,
    goal: u32,
    beam: u32,
}

impl Case {
    fn solve(&self, label: &str) -> (i64, Vec<u32>) {
        self.solve_with(table(label))
    }

    fn solve_with(&self, evaluator: &dyn Evaluator) -> (i64, Vec<u32>) {
        let game = generate_game(self.seed, self.pieces, self.cap)
            .unwrap_or_else(|error| panic!("case {self:?} is out of domain: {error:?}"));
        let result = solve(
            &game.board,
            game.sequence.as_slice(),
            &SolveConfig::new(self.goal, self.beam),
            evaluator,
        );
        (
            result.packed(),
            result.moves.iter().map(|mv| mv.packed()).collect(),
        )
    }
}

fn cases() -> Vec<Case> {
    let path = fixture_path("parity-games.txt");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is not readable: {error}", path.display()));
    let mut cases = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let numbers: Vec<u32> = line
            .split_whitespace()
            .map(|field| field.parse().expect("a decimal number"))
            .collect();
        assert_eq!(
            numbers.len(),
            5,
            "`{line}` is not `seed pieces cap goal beam`"
        );
        cases.push(Case {
            seed: numbers[0],
            pieces: numbers[1],
            cap: numbers[2],
            goal: numbers[3],
            beam: numbers[4],
        });
    }
    assert!(
        cases.len() >= 4,
        "the parity set shrank to {} cases",
        cases.len()
    );
    cases
}

fn moves_field(moves: &[u32]) -> String {
    if moves.is_empty() {
        return "-".to_string();
    }
    moves
        .iter()
        .map(|word| format!("{word:08x}"))
        .collect::<Vec<String>>()
        .join(",")
}

/// One committed row: the case, its table label, §4.2's packed result and the Move
/// list. The report's order is the order the cases and tables are written in; the
/// test compares by key, not by line.
type Row = (Case, String, i64, Vec<u32>);

fn report_rows() -> (Option<String>, Vec<Row>) {
    let path = fixture_path("parity-report.txt");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is not readable: {error}", path.display()));
    let mut marker = None;
    let mut rows = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(label) = line.strip_prefix("shipped ") {
            marker = Some(label.trim().to_string());
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(f.len(), 8, "`{line}` is not a parity row");
        let case = Case {
            seed: f[0].parse().unwrap(),
            pieces: f[1].parse().unwrap(),
            cap: f[2].parse().unwrap(),
            goal: f[3].parse().unwrap(),
            beam: f[4].parse().unwrap(),
        };
        let packed: i64 = f[6].parse().unwrap();
        let moves: Vec<u32> = if f[7] == "-" {
            Vec::new()
        } else {
            f[7].split(',')
                .map(|word| u32::from_str_radix(word, 16).expect("a packed Move"))
                .collect()
        };
        rows.push((case, f[5].to_string(), packed, moves));
    }
    (marker, rows)
}

#[test]
fn the_parity_set_produces_the_committed_scores_and_move_lists() {
    let (marker, rows) = report_rows();
    assert!(marker.is_some(), "the report names the shipped table");
    assert_eq!(
        rows.len(),
        cases().len() * TABLES.len(),
        "one row per case and table"
    );

    for (case, label, packed, moves) in rows {
        let (actual_packed, actual_moves) = case.solve(&label);
        assert_eq!(
            actual_packed, packed,
            "seed {} pieces {} cap {} goal {} beam {} `{}`: packed result",
            case.seed, case.pieces, case.cap, case.goal, case.beam, label
        );
        assert_eq!(
            moves_field(&actual_moves),
            moves_field(&moves),
            "seed {} pieces {} cap {} goal {} beam {} `{}`: Move list",
            case.seed,
            case.pieces,
            case.cap,
            case.goal,
            case.beam,
            label
        );
    }
}

#[test]
fn the_parity_report_ships_the_evaluator_the_engine_ships() {
    // The marker is what routes the node suite to the same rows, so a §9.4 verdict
    // that forgets to regenerate the report fails here instead of passing there.
    let (marker, _) = report_rows();
    let marker = marker.expect("the report names the shipped table");
    let shipped_name = shipped().name();
    let expected = TABLES
        .iter()
        .find(|label| table(label).name() == shipped_name)
        .unwrap_or_else(|| panic!("`{shipped_name}` is not in the report"));
    assert_eq!(&marker, expected, "the report's `shipped` line");
}

#[test]
fn the_parity_set_can_tell_the_two_weight_tables_apart() {
    // Without this, comparing Move lists would be comparing two runs of the same
    // arithmetic and would say nothing about scores. At Beam width 1 the search is
    // a greedy arg-max, and at width 8 a one-unit score difference changes the cut.
    let mut differs = 0;
    for case in cases() {
        if case.solve("e0") != case.solve("e1") {
            differs += 1;
        }
    }
    assert!(
        differs >= 2,
        "only {differs} of the parity cases distinguish E0 from E1 — the set has stopped observing scores"
    );
}

/// How sharp the comparison is, measured: one Q16 unit is 1/65536 of a weight, the
/// finest step a committed table can take. Two of E1's eight weights already move
/// the play at that step (measured: `wells` −1 changed 4 of 5 cases, `eroded_cells`
/// −1 changed 3), so a Move list that matches across the two builds is not merely
/// agreeing about legality — a porting bug cannot hide in a score.
#[test]
fn the_parity_set_resolves_a_one_unit_weight_change() {
    let cases = cases();
    for (feature, minimum) in [("wells", 3usize), ("eroded_cells", 2)] {
        let mut weights = E1_PUBLISHED.as_array();
        let index = engine::evaluator::DtWeights::FEATURES
            .iter()
            .position(|name| *name == feature)
            .expect("a committed feature name");
        weights[index] -= 1;
        let nudged = DellacherieThiery(engine::evaluator::DtWeights::from_array(weights));
        let mut changed = 0;
        for case in &cases {
            if case.solve_with(&nudged) != case.solve("e1") {
                changed += 1;
            }
        }
        assert!(
            changed >= minimum,
            "`{feature}` moved by 1/65536 changed only {changed} of {} cases",
            cases.len()
        );
    }
}

/// Writes the committed golden. Run it only when a change is *meant* to move
/// scores — a weight table, a feature definition, the search's visit order — and
/// read the diff before committing it: the file is a fingerprint, not a proof.
///
///   cargo test -p engine --test parity -- --ignored --nocapture bless_the_parity_report
#[test]
#[ignore = "writes tests/fixtures/parity-report.txt"]
fn bless_the_parity_report() {
    let mut out = String::new();
    out.push_str(
        "# Native/wasm score parity: what `cargo test` and the built wasm must agree on.\n",
    );
    out.push_str("#\n");
    out.push_str(
        "# One row per case and committed weight table: the Seed, piece count and height cap\n",
    );
    out.push_str(
        "# of the Board and Sequence (§5.5), the Goal and Beam they are solved at, the table,\n",
    );
    out.push_str(
        "# §4.2's packed result as a decimal `i64`, and the Move list as packed words in hex\n",
    );
    out.push_str(
        "# (`-` when the Solve returned no moves). Read by `engine/tests/parity.rs` and by\n",
    );
    out.push_str(
        "# `web/test/score-parity.test.mjs`; the `shipped` line routes both to the same rows.\n",
    );
    out.push('\n');
    let shipped_name = shipped().name();
    let marker = TABLES
        .iter()
        .find(|label| table(label).name() == shipped_name)
        .expect("the shipped evaluator is in the report");
    out.push_str(&format!("shipped {marker}\n\n"));
    for case in cases() {
        for label in TABLES {
            let (packed, moves) = case.solve(label);
            out.push_str(&format!(
                "{} {} {} {} {} {} {} {}\n",
                case.seed,
                case.pieces,
                case.cap,
                case.goal,
                case.beam,
                label,
                packed,
                moves_field(&moves)
            ));
        }
    }
    let path = fixture_path("parity-report.txt");
    std::fs::write(&path, out)
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", path.display()));
    println!("wrote {}", path.display());
}
