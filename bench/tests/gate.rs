//! Seam 3: the six Gates (§9.2, ticket 03).
//!
//! Every test here calls the same functions `cargo run -p bench -- gate all`
//! calls, so a green suite means the gate a developer types and the gate CI runs
//! are one implementation. Three of them are the fire drill in miniature: a
//! deliberately broken Dataset copy must turn the gate red *and name the game*,
//! a corrupted Move list must be refused by the independent simulator, and a
//! missing or malformed golden file must fail gate 6 rather than pass by accident
//! (ticket 09 replaced ticket 03's placeholder with the exact compare; the compare
//! itself is exercised in `bench/tests/golden.rs`).
//!
//! Gates 4 and 6 read build artifacts (`web/pkg/engine.wasm`, `web/dist`) that
//! may not exist in a bare checkout. They assert the honest thing either way:
//! pass with the numbers printed when the artifact is there, and a named
//! `cannot run` with the command that builds it when it is not.

use std::fs;
use std::path::{Path, PathBuf};

use bench::dataset::{self, Config};
use bench::gate::{self, Check, Options, Paths, Report, State};
use bench::replay;

// ---------------------------------------------------------------- fixtures

fn options() -> Options {
    Options {
        paths: Paths::resolve(),
        exe: PathBuf::from(env!("CARGO_BIN_EXE_bench")),
    }
}

/// A temp directory holding its own copy of `data/dataset`, so a test can break
/// the Dataset without touching the committed one.
fn scratch_dataset(tag: &str) -> Paths {
    let mut dir = std::env::temp_dir();
    dir.push(format!("tetris-lab-03-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let source = Paths::resolve().data;
    for cfg in dataset::CONFIGS {
        let from = dataset::config_dir(&source, &cfg);
        let to = dataset::config_dir(&dir, &cfg);
        fs::create_dir_all(&to).unwrap();
        for kind in dataset::Kind::ALL {
            let file = from.join(kind.file());
            if file.exists() {
                fs::copy(&file, to.join(kind.file())).unwrap();
            }
        }
    }
    let mut paths = Paths::resolve();
    paths.data = dir;
    paths
}

fn games_file(paths: &Paths, cfg: &Config) -> PathBuf {
    dataset::games_path(&paths.data, cfg)
}

fn manifest_file(paths: &Paths, cfg: &Config) -> PathBuf {
    dataset::manifest_path(&paths.data, cfg)
}

/// Flip one bit of one column word of one committed game, in place: the exact
/// break the fire drill makes, and the one §9.2 gate 1 exists to catch. Returns
/// the word before and after.
fn flip_one_bit(path: &Path, seed: u32, column: usize) -> (String, String) {
    let text = fs::read_to_string(path).unwrap();
    let mut out = String::with_capacity(text.len());
    let mut before = String::new();
    let mut after = String::new();
    for line in text.lines() {
        let mut fields: Vec<String> = line.split_whitespace().map(str::to_string).collect();
        if fields.len() == 12 && fields[0] == seed.to_string() {
            before = fields[1 + column].clone();
            let word = u32::from_str_radix(&before, 16).unwrap() ^ 0x20;
            after = format!("{word:08x}");
            fields[1 + column] = after.clone();
            out.push_str(&fields.join(" "));
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    assert!(
        !before.is_empty(),
        "seed {seed} is not in {}",
        path.display()
    );
    fs::write(path, out).unwrap();
    (before, after)
}

/// Remove one game line, so the file jumps over a seed.
fn drop_a_game_line(path: &Path, seed: u32) {
    let text = fs::read_to_string(path).unwrap();
    let mut out = String::with_capacity(text.len());
    let mut removed = false;
    for line in text.lines() {
        if line.split_whitespace().next() == Some(seed.to_string().as_str()) {
            removed = true;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    assert!(removed, "seed {seed} is not in {}", path.display());
    fs::write(path, out).unwrap();
}

fn check<'a>(report: &'a Report, needle: &str) -> &'a Check {
    report
        .checks
        .iter()
        .find(|check| check.name.contains(needle))
        .unwrap_or_else(|| {
            panic!(
                "gate {} has no assertion named {needle:?}; it has:\n{}",
                report.gate,
                report
                    .checks
                    .iter()
                    .map(|check| format!("  {}", check.name))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        })
}

// -------------------------------------------------- gate 1, the fingerprint

#[test]
fn gate_1_passes_on_the_committed_dataset() {
    let report = gate::run(1, &options());
    let rendered = report.render();
    assert_eq!(report.exit_code(), 0, "{rendered}");
    assert!(report.passed(), "{rendered}");
    // Both configurations, regenerated bit for bit, and both digests printed:
    // the two numbers every other figure is keyed on.
    for (cfg, digest) in [
        (dataset::GOAL8.dir, "4bf1a17b1685a774"),
        (dataset::GOAL12.dir, "4ebdf44940134c40"),
    ] {
        assert!(
            rendered.contains(&format!("{cfg} · regenerates bit for bit")),
            "{rendered}"
        );
        assert!(rendered.contains(digest), "{cfg}: {rendered}");
    }
    assert_eq!(check(&report, "regenerates bit for bit").state, State::Pass);
    assert!(rendered.contains("2000 games"), "{rendered}");
}

#[test]
fn gate_1_names_the_game_that_moved_bit_for_bit() {
    let paths = scratch_dataset("gate1");
    let (before, after) = flip_one_bit(&games_file(&paths, &dataset::GOAL8), 512, 3);

    let report = gate::run(
        1,
        &Options {
            paths,
            exe: options().exe,
        },
    );
    let rendered = report.render();

    assert_eq!(report.exit_code(), 1, "{rendered}");
    assert!(!report.passed(), "{rendered}");
    // The first mismatch names the game, not the value: seed, position in the
    // file, the field in the file's own vocabulary, and both values.
    assert!(rendered.contains("seed 512"), "{rendered}");
    assert!(rendered.contains("game 513 of 2000"), "{rendered}");
    assert!(rendered.contains("column word 3"), "{rendered}");
    assert!(rendered.contains(&before), "{rendered}");
    assert!(rendered.contains(&after), "{rendered}");
    // Both digests are printed even when exactly one game moved.
    assert!(rendered.contains("4bf1a17b1685a774"), "{rendered}");
    assert!(
        rendered.contains("games that differ: 1 of 2000"),
        "{rendered}"
    );
    // And the failure names itself: a pasted log line says which gate broke.
    assert!(rendered.contains("gate 1 ·"), "{rendered}");
    // The other configuration is untouched and still passes.
    assert_eq!(
        check(&report, dataset::GOAL12.dir).state,
        State::Pass,
        "{rendered}"
    );
}

// ------------------------------------------------ gate 2, manifest agreement

#[test]
fn gate_2_passes_on_the_committed_dataset() {
    let report = gate::run(2, &options());
    let rendered = report.render();
    assert!(report.passed(), "{rendered}");
    assert_eq!(report.exit_code(), 0, "{rendered}");
    for cfg in dataset::CONFIGS {
        for split in ["test", "tuning"] {
            let name = format!("{} · split {split}", cfg.dir);
            assert_eq!(check(&report, &name).state, State::Pass, "{rendered}");
        }
    }
    // Twenty assertions over two configurations and two splits; the count is
    // part of the report so a dropped check is visible.
    assert!(report.counted() >= 16, "{rendered}");
    assert!(
        rendered.contains("0-999") && rendered.contains("1000-1999"),
        "{rendered}"
    );
}

#[test]
fn gate_2_names_a_manifest_field_that_disagrees() {
    let paths = scratch_dataset("gate2-field");
    let path = manifest_file(&paths, &dataset::GOAL8);
    let text = fs::read_to_string(&path)
        .unwrap()
        .replace("\"goal\": 8", "\"goal\": 9");
    fs::write(&path, text).unwrap();

    let report = gate::run(
        2,
        &Options {
            paths,
            exe: options().exe,
        },
    );
    let rendered = report.render();
    assert_eq!(report.exit_code(), 1, "{rendered}");
    assert!(rendered.contains("manifest field goal"), "{rendered}");
    assert!(rendered.contains("gate 2 ·"), "{rendered}");
}

#[test]
fn gate_2_names_seed_coverage_that_disagrees_with_the_manifest() {
    // Dropping the last game leaves a file that still parses (0-1998, ascending)
    // but no longer covers the seeds the manifest declares.
    let paths = scratch_dataset("gate2-coverage");
    drop_a_game_line(&games_file(&paths, &dataset::GOAL8), 1999);

    let report = gate::run(
        2,
        &Options {
            paths,
            exe: options().exe,
        },
    );
    let rendered = report.render();
    assert_eq!(report.exit_code(), 1, "{rendered}");
    assert!(rendered.contains("seed coverage"), "{rendered}");
    assert!(rendered.contains("1999 games, expected 2000"), "{rendered}");
    assert!(rendered.contains("last seed is 1999"), "{rendered}");
}

#[test]
fn gate_2_surfaces_a_file_it_cannot_parse() {
    // A gap in the middle is refused by the parser, and gate 2 must report that
    // refusal as a named assertion rather than panic or pass.
    let paths = scratch_dataset("gate2-parse");
    drop_a_game_line(&games_file(&paths, &dataset::GOAL8), 500);

    let report = gate::run(
        2,
        &Options {
            paths,
            exe: options().exe,
        },
    );
    let rendered = report.render();
    assert_eq!(report.exit_code(), 1, "{rendered}");
    assert!(rendered.contains("does not parse"), "{rendered}");
    assert!(rendered.contains("line 502"), "{rendered}");
    assert!(rendered.contains("501"), "{rendered}");
}

// --------------------------------------------------- gate 3, replay validity

#[test]
fn gate_3_every_returned_move_list_replays() {
    let report = gate::run(3, &options());
    let rendered = report.render();
    assert!(report.passed(), "{rendered}");
    for cfg in dataset::CONFIGS {
        for name in ["replay", "lines", "losses are empty"] {
            let full = format!("{} · {name}", cfg.dir);
            assert_eq!(check(&report, &full).state, State::Pass, "{rendered}");
        }
    }
    // 100 seeds per configuration, and the report says how many were won.
    assert!(rendered.contains("100"), "{rendered}");
}

#[test]
fn gate_3_reports_a_move_list_that_does_not_replay() {
    // A real solve, then three deliberate corruptions of its Move list. The
    // independent simulator must refuse each one and name the move.
    let cfg = dataset::GOAL8;
    let game = engine::generate_game(0, cfg.pieces, cfg.cap).unwrap();
    let sequence = game.sequence.as_slice();
    let result = engine::solve(
        &game.board,
        sequence,
        &engine::SolveConfig::new(cfg.goal, gate::GATE_BEAM),
        engine::shipped(),
    );
    assert!(result.won, "seed 0 must win at goal 8, width 25");
    assert!(result.moves.len() >= 2);

    // The untouched list replays to the claimed lines.
    let replayed = replay::replay(&game.board, sequence, &result.moves).unwrap();
    assert_eq!(replayed.lines, result.lines);
    assert_eq!(replayed.moves, result.moves.len());
    assert_eq!(replayed.full_rows, 0, "a replay leaves no full row");

    let mut wrong_piece = result.moves.clone();
    wrong_piece[1].piece_index = (wrong_piece[1].piece_index + 1) % 7;
    let error = replay::replay(&game.board, sequence, &wrong_piece).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("move 2"), "{message}");
    assert!(message.contains("piece"), "{message}");

    let mut off_field = result.moves.clone();
    off_field[0].column = 40;
    let message = replay::replay(&game.board, sequence, &off_field)
        .unwrap_err()
        .to_string();
    assert!(message.contains("move 1"), "{message}");

    let mut too_many = result.moves.clone();
    while too_many.len() <= cfg.pieces as usize {
        too_many.extend_from_slice(&result.moves);
    }
    let message = replay::replay(&game.board, sequence, &too_many)
        .unwrap_err()
        .to_string();
    assert!(message.contains("more moves than pieces"), "{message}");

    // A list that is legal but shorter than the claim is caught by the caller:
    // the simulator reports the lines it actually cleared.
    let short = replay::replay(&game.board, sequence, &result.moves[..1]).unwrap();
    assert!(short.lines <= result.lines);
}

// -------------------------------------------------------- gate 4, parity

#[test]
fn gate_4_compares_native_against_the_real_artifact() {
    let options = options();
    if !options.paths.wasm.exists() {
        // A bare checkout: the honest answer is "cannot run", never a pass.
        let report = gate::run(4, &options);
        assert_eq!(report.exit_code(), 2, "{}", report.render());
        assert!(
            report.render().contains("cannot run"),
            "{}",
            report.render()
        );
        return;
    }
    let report = gate::run(4, &options);
    let rendered = report.render();
    assert!(report.passed(), "{rendered}");
    for cfg in dataset::CONFIGS {
        for field in ["status", "won", "evaluations", "lines"] {
            let name = format!("{} · parity · {field}", cfg.dir);
            assert_eq!(check(&report, &name).state, State::Pass, "{rendered}");
        }
    }
    assert!(rendered.contains("100 seeds"), "{rendered}");
}

#[test]
fn gate_4_cannot_run_without_the_artifact() {
    let mut paths = Paths::resolve();
    paths.wasm = paths.data.join("no-such-engine.wasm");
    let report = gate::run(
        4,
        &Options {
            paths,
            exe: options().exe,
        },
    );
    let rendered = report.render();
    assert_eq!(report.exit_code(), 2, "{rendered}");
    assert!(!report.passed(), "{rendered}");
    assert!(rendered.contains("cannot run"), "{rendered}");
    assert!(rendered.contains("build:wasm"), "{rendered}");
}

// ----------------------------------------------------- gate 5, determinism

#[test]
fn gate_5_determinism_in_one_process_and_across_two() {
    let report = gate::run(5, &options());
    let rendered = report.render();
    assert!(report.passed(), "{rendered}");
    assert_eq!(report.exit_code(), 0, "{rendered}");
    for cfg in dataset::CONFIGS {
        let name = format!("{} · same seed in one process", cfg.dir);
        assert_eq!(check(&report, &name).state, State::Pass, "{rendered}");
    }
    let across = check(&report, "two processes");
    assert_eq!(across.state, State::Pass, "{rendered}");
    // The five fixed seeds are named in the report, so a rerun is reproducible.
    for seed in gate::DETERMINISM_SEEDS {
        assert!(rendered.contains(&seed.to_string()), "{rendered}");
    }
}

// ---------------------------------------------------------- gate 6, budgets

#[test]
fn gate_6_reports_every_budget_it_measured() {
    let report = gate::run(6, &options());
    let rendered = report.render();
    assert_eq!(report.exit_code(), 0, "{rendered}");
    // The raw wasm size, and the page gzip total with its components.
    let wasm = check(&report, "wasm raw size");
    assert_eq!(wasm.state, State::Pass, "{rendered}");
    let gzip = check(&report, "page gzip");
    assert_eq!(gzip.state, State::Pass, "{rendered}");
    assert!(rendered.contains("warn"), "{rendered}");
    assert!(
        rendered.contains(&gate::WASM_FAIL_BYTES.to_string()),
        "{rendered}"
    );
    assert!(
        rendered.contains(&gate::PAGE_FAIL_BYTES.to_string()),
        "{rendered}"
    );
    // A warn never blocks: the 100 KiB line is reported, the exit code is 0.
    assert!(
        rendered.contains("102400") || rendered.contains("100 KiB"),
        "{rendered}"
    );
}

#[test]
fn gate_6_fails_over_the_fail_line_and_names_the_budget() {
    // The budget is a threshold, so it is tested as one: a copy of the artifact
    // padded past 150 KiB must fail gate 6 naming the size budget.
    let mut paths = Paths::resolve();
    if !paths.wasm.exists() {
        return;
    }
    let mut big = std::env::temp_dir();
    big.push(format!("tetris-lab-03-big-{}.wasm", std::process::id()));
    let mut bytes = fs::read(&paths.wasm).unwrap();
    bytes.resize(gate::WASM_FAIL_BYTES as usize + 1, 0);
    fs::write(&big, &bytes).unwrap();
    paths.wasm = big.clone();

    let report = gate::run(
        6,
        &Options {
            paths,
            exe: options().exe,
        },
    );
    let rendered = report.render();
    let _ = fs::remove_file(&big);
    assert_eq!(report.exit_code(), 1, "{rendered}");
    assert_eq!(
        check(&report, "wasm raw size").state,
        State::Fail,
        "{rendered}"
    );
    assert!(rendered.contains("153600"), "{rendered}");
}

#[test]
fn gate_6_fails_when_no_golden_is_committed() {
    // There is no placeholder any more: after ticket 09 the golden *is* committed,
    // so its absence is a red gate that says how to pin one — never a `·` line.
    let mut paths = Paths::resolve();
    // Point at a measurements directory that certainly has no golden file.
    paths.measurements = std::env::temp_dir().join("tetris-lab-03-no-measurements");
    let report = gate::run(
        6,
        &Options {
            paths,
            exe: options().exe,
        },
    );
    let rendered = report.render();
    let golden = check(&report, "golden evaluations");
    assert_eq!(golden.state, State::Fail, "{rendered}");
    assert!(rendered.contains("no golden is committed"), "{rendered}");
    assert!(rendered.contains("bench golden update"), "{rendered}");
    assert_eq!(report.exit_code(), 1, "{rendered}");
}

#[test]
fn gate_6_compares_the_committed_golden_exactly() {
    // The committed golden, the committed Dataset and the built artifact: the exact
    // compare is green, and it says the digest and the seed count out loud.
    let report = gate::run(6, &options());
    let rendered = report.render();
    let golden = check(&report, "golden identity");
    assert_eq!(golden.state, State::Pass, "{rendered}");
    let evals = check(&report, "golden evaluations · goal8-p40-cap7");
    assert_eq!(evals.state, State::Pass, "{rendered}");
    assert!(evals.subject.contains("of 1000 seeds"), "{rendered}");
    let lines = check(&report, "golden lines · goal12-p100-cap7");
    assert_eq!(lines.state, State::Pass, "{rendered}");
    let clock = check(&report, "golden wall clock · goal8-p40-cap7");
    assert_eq!(clock.state, State::Pass, "{rendered}");
    assert!(clock.subject.contains("normalised"), "{rendered}");
    // Nothing of gate 6b is skipped: the placeholder is gone.
    assert!(report.skipped() == 0, "{rendered}");
}

#[test]
fn gate_6_refuses_a_golden_it_cannot_read() {
    // A `{}` cannot be compared against anything, and it must not be able to look
    // like a green gate: the file is named, and so is the field that is missing.
    let mut paths = Paths::resolve();
    let mut dir = std::env::temp_dir();
    dir.push(format!("tetris-lab-03-golden-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(gate::GOLDEN_FILE), "{}\n").unwrap();
    paths.measurements = dir.clone();

    let report = gate::run(
        6,
        &Options {
            paths,
            exe: options().exe,
        },
    );
    let rendered = report.render();
    let golden = check(&report, "golden evaluations");
    assert_eq!(golden.state, State::Fail, "{rendered}");
    assert_eq!(report.exit_code(), 1, "{rendered}");
    assert!(rendered.contains("format"), "{rendered}");
    let _ = fs::remove_dir_all(&dir);
}

// ------------------------------------------------------------ the suite

#[test]
fn every_gate_names_itself_and_its_assertions() {
    let options = options();
    for gate_number in gate::GATES {
        let report = gate::run(gate_number, &options);
        assert_eq!(report.gate, gate_number);
        assert!(!report.title.is_empty(), "gate {gate_number} has no title");
        assert!(
            !report.checks.is_empty(),
            "gate {gate_number} asserted nothing"
        );
        let rendered = report.render();
        assert!(
            rendered.starts_with(&format!("gate {gate_number} · ")),
            "{rendered}"
        );
        // Every summary line names the gate, so a pasted ✗ is self-identifying.
        if !report.passed() {
            assert!(
                rendered.contains(&format!("✗ gate {gate_number} ·")),
                "{rendered}"
            );
        }
    }
}

#[test]
fn gate_all_runs_all_six_in_order() {
    let reports = gate::run_all(&options());
    assert_eq!(reports.len(), 6);
    for (index, report) in reports.iter().enumerate() {
        assert_eq!(report.gate, index as u8 + 1);
    }
    let worst = gate::worst_exit_code(&reports);
    assert_eq!(
        worst,
        0,
        "{}",
        reports.iter().map(Report::render).collect::<String>()
    );
}
