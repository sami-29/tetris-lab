//! Seam 1: the Generator's output as committed data (ticket 02, spec §6, §9.2
//! gate 1).
//!
//! Every test here reads the real committed Dataset under `data/dataset/`. The
//! two that need to write (drift, archiving) work on a copy in the temp dir, so
//! a red test can never damage the committed files.
//!
//! `verify::check_config` / `verify::run_all` are the same functions the CI gate
//! runs (`cargo run -p bench --release -- verify`); these tests are how the gate
//! is itself checked.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use bench::dataset::{self, Config, GameLine, Manifest};
use bench::{export, fingerprint, verify};
use engine::Board;

// ---------------------------------------------------------------- fixtures

fn load(cfg: &Config) -> dataset::Parsed {
    let path = dataset::games_path(&dataset::root(), cfg);
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    dataset::parse(cfg, &text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

fn load_manifest(cfg: &Config) -> Manifest {
    load_manifest_at(&dataset::root(), cfg)
}

fn load_manifest_at(data: &Path, cfg: &Config) -> Manifest {
    let path = dataset::manifest_path(data, cfg);
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    Manifest::parse(&text).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

/// A fresh, empty directory under the temp dir; the whole `data/dataset` tree
/// gets copied into it so tests can mutate a Dataset without touching the real
/// one.
fn scratch_root(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("tetris-lab-02-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn copy_config(from: &Path, to: &Path, cfg: &Config) {
    let src = dataset::config_dir(from, cfg);
    let dst = dataset::config_dir(to, cfg);
    fs::create_dir_all(&dst).unwrap();
    for kind in dataset::Kind::ALL {
        let from = src.join(kind.file());
        let to = dst.join(kind.file());
        if from.exists() {
            fs::copy(&from, &to).unwrap();
        }
    }
}

/// The line of a failure message that carries `key`, so formatting (the column
/// alignment in the message) is not what the test asserts.
fn field_line<'a>(message: &'a str, key: &str) -> &'a str {
    message
        .lines()
        .find(|line| line.contains(key))
        .unwrap_or("")
}

// ------------------------------------------------------- T1 manifest agrees

#[test]
fn manifest_agrees_with_its_file() {
    for cfg in dataset::CONFIGS {
        let parsed = load(&cfg);
        let manifest = load_manifest(&cfg);
        let where_ = cfg.dir;

        assert_eq!(manifest.format, dataset::FORMAT, "{where_}: format");
        assert_eq!(manifest.goal, cfg.goal, "{where_}: goal vs directory");
        assert_eq!(manifest.pieces, cfg.pieces, "{where_}: pieces vs directory");
        assert_eq!(manifest.cap, cfg.cap, "{where_}: cap vs directory");

        // Header and manifest are two renderings of one configuration.
        assert_eq!(
            manifest.goal, parsed.header.goal,
            "{where_}: goal vs header"
        );
        assert_eq!(
            manifest.pieces, parsed.header.pieces,
            "{where_}: pieces vs header"
        );
        assert_eq!(manifest.cap, parsed.header.cap, "{where_}: cap vs header");
        assert_eq!(
            manifest.seeds,
            (parsed.header.first_seed, parsed.header.last_seed),
            "{where_}: seeds vs header"
        );

        // Both splits, present and disjoint, covering the seeds.
        assert_eq!(
            manifest.test_split,
            dataset::TEST_SPLIT,
            "{where_}: test split"
        );
        assert_eq!(
            manifest.tuning_split,
            dataset::TUNING_SPLIT,
            "{where_}: tuning split"
        );
        assert_eq!(
            manifest.test_split.1 + 1,
            manifest.tuning_split.0,
            "{where_}: the splits must be adjacent and disjoint"
        );
        assert!(manifest.test_split.0 <= manifest.test_split.1, "{where_}");
        assert_eq!(manifest.seeds.0, manifest.test_split.0, "{where_}");
        assert_eq!(manifest.seeds.1, manifest.tuning_split.1, "{where_}");

        // Every seed exactly once, ascending, contiguous.
        assert_eq!(
            manifest.game_count as usize,
            parsed.games.len(),
            "{where_}: game_count vs lines"
        );
        assert_eq!(
            manifest.game_count,
            manifest.seeds.1 - manifest.seeds.0 + 1,
            "{where_}: game_count vs seed range"
        );
        for (offset, game) in parsed.games.iter().enumerate() {
            assert_eq!(
                game.seed,
                manifest.seeds.0 + offset as u32,
                "{where_}: seed order/contiguity"
            );
        }

        // The description is one line, and the fingerprint is the §6.1 shape.
        assert!(
            !manifest.generator_description.is_empty()
                && !manifest.generator_description.contains('\n'),
            "{where_}: generator_description must be one non-empty line"
        );
        assert!(
            manifest.fingerprint.len() == 16
                && manifest
                    .fingerprint
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "{where_}: fingerprint must be 16 lowercase hex digits, got {:?}",
            manifest.fingerprint
        );
        assert!(
            manifest.generator_version >= 1,
            "{where_}: generator_version"
        );
    }
}

// -------------------------------------------------------- T2 canonical header

#[test]
fn header_line_is_canonical() {
    for cfg in dataset::CONFIGS {
        let path = dataset::games_path(&dataset::root(), &cfg);
        let text = fs::read_to_string(&path).unwrap();
        let first = text.lines().next().expect("games.txt is not empty");
        let expected = dataset::header_line(&cfg);
        assert_eq!(
            first, expected,
            "{}: header line is not canonical\n  expected: {expected:?}\n  actual:   {first:?}",
            cfg.dir
        );
        assert_eq!(
            first,
            expected.trim_end(),
            "{}: trailing whitespace",
            cfg.dir
        );
        assert!(
            !first.bytes().any(|b| b == b'\r'),
            "{}: header carries a CR",
            cfg.dir
        );
    }
}

// ------------------------------------------ T3 every game regenerates bit for bit

#[test]
fn every_committed_game_regenerates_bit_for_bit() {
    for cfg in dataset::CONFIGS {
        let parsed = load(&cfg);
        for game in &parsed.games {
            let fresh = engine::generate_game(game.seed, cfg.pieces, cfg.cap)
                .unwrap_or_else(|e| panic!("{}: seed {} refused: {e:?}", cfg.dir, game.seed));
            let expected = GameLine::from_engine(&fresh);
            assert_eq!(
                expected.board, game.board,
                "{}: seed {} moved (board)\n  expected: {:?}\n  actual:   {:?}",
                cfg.dir, game.seed, expected.board, game.board
            );
            assert_eq!(
                expected.sequence, game.sequence,
                "{}: seed {} moved (sequence)",
                cfg.dir, game.seed
            );
        }
    }
}

// ------------------------------- T3b the failure names the first game that moved

#[test]
fn drift_names_the_first_game_that_moved() {
    let root = scratch_root("drift");
    for cfg in dataset::CONFIGS {
        copy_config(&dataset::root(), &root, &cfg);
    }

    // Move one bit of one board word of one game: seed 512, column word 3.
    let cfg = dataset::GOAL8;
    let path = dataset::games_path(&root, &cfg);
    let text = fs::read_to_string(&path).unwrap();
    let before = dataset::parse(&cfg, &text).unwrap();
    let word = before.games[512].board[3];
    let moved = word ^ 0x20;

    let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let line_index = 512 + 1; // header, then seed order
    assert!(
        lines[line_index].starts_with("512 "),
        "expected seed 512 on line {line_index}, got {:?}",
        lines[line_index]
    );
    let fields: Vec<&str> = lines[line_index].split(' ').collect();
    let mut rebuilt = String::new();
    for (i, field) in fields.iter().enumerate() {
        if i > 0 {
            rebuilt.push(' ');
        }
        if i == 4 {
            rebuilt.push_str(&format!("{moved:08x}"));
        } else {
            rebuilt.push_str(field);
        }
    }
    lines[line_index] = rebuilt;
    fs::write(&path, lines.join("\n") + "\n").unwrap();

    let drift =
        verify::check_config(&cfg, &root).expect_err("a moved board must make the gate red");
    let message = drift.to_string();

    assert!(
        message.contains("games.txt"),
        "message names the file: {message}"
    );
    assert!(
        message.contains("seed 512"),
        "message names the seed: {message}"
    );
    assert!(
        message.contains("game 513 of 2000"),
        "message names the position in seed order: {message}"
    );
    assert!(
        message.contains("line 514"),
        "message names the line: {message}"
    );
    assert!(
        message.contains("column word 3"),
        "message names the field: {message}"
    );
    assert!(
        field_line(&message, "expected:").contains(&format!("{word:08x}")),
        "message quotes the regenerated word: {message}"
    );
    assert!(
        field_line(&message, "actual:").contains(&format!("{moved:08x}")),
        "message quotes the committed word: {message}"
    );
    assert!(
        message.contains("1 of 2000"),
        "message counts the games that moved: {message}"
    );
    // Both digests are always printed, so the value is comparable by eye.
    let manifest = load_manifest(&cfg);
    assert!(
        message.contains(&manifest.fingerprint),
        "message quotes the manifest fingerprint: {message}"
    );
    assert!(
        message.contains("data/dataset/README.md"),
        "message points at the legitimate-change procedure: {message}"
    );

    // The gate is not red before the edit: the fixture is the committed file.
    let restored = scratch_root("drift-clean");
    for cfg in dataset::CONFIGS {
        copy_config(&dataset::root(), &restored, &cfg);
    }
    assert!(verify::check_config(&dataset::GOAL8, &restored).is_ok());
    let _ = fs::remove_dir_all(&root);
    let _ = fs::remove_dir_all(&restored);
}

// -------------------------------- T4/T5 the fingerprint, from file and from seed

#[test]
fn committed_fingerprint_matches_manifest() {
    for cfg in dataset::CONFIGS {
        let parsed = load(&cfg);
        let manifest = load_manifest(&cfg);
        let computed = fingerprint::hex(fingerprint::fingerprint(&parsed.games));
        assert_eq!(
            computed, manifest.fingerprint,
            "{}: fingerprint of the committed file does not match its manifest",
            cfg.dir
        );
    }
}

#[test]
fn regenerated_fingerprint_matches_manifest() {
    for cfg in dataset::CONFIGS {
        let manifest = load_manifest(&cfg);
        let mut fresh = Vec::with_capacity(dataset::GAME_COUNT);
        for seed in dataset::SEED_FIRST..=dataset::SEED_LAST {
            let game = engine::generate_game(seed, cfg.pieces, cfg.cap).unwrap();
            fresh.push(GameLine::from_engine(&game));
        }
        let computed = fingerprint::hex(fingerprint::fingerprint(&fresh));
        assert_eq!(
            computed, manifest.fingerprint,
            "{}: the engine no longer makes the committed games",
            cfg.dir
        );
    }
}

#[test]
fn the_committed_fingerprints_are_the_frozen_goldens() {
    // These two numbers are the Dataset's identity. They may only move through
    // the §6.2 procedure (`data/dataset/README.md`): archive the old file under
    // its fingerprint, bump generator_version, record the reason, add a
    // Reference row. If this test is red and you did not change the generator,
    // the file is wrong.
    let goldens = [
        (dataset::GOAL8, "4bf1a17b1685a774"),
        (dataset::GOAL12, "4ebdf44940134c40"),
    ];
    for (cfg, golden) in goldens {
        let manifest = load_manifest(&cfg);
        assert_eq!(
            manifest.fingerprint, golden,
            "{}: the committed fingerprint moved; the golden is the released value",
            cfg.dir
        );
    }
}

// ---------------------------------------- T6 the fingerprint is semantic not textual

#[test]
fn the_fingerprint_is_semantic_not_textual() {
    for cfg in dataset::CONFIGS {
        let path = dataset::games_path(&dataset::root(), &cfg);
        let text = fs::read_to_string(&path).unwrap();
        let reformatted = reformat(&text);

        // The reformat really did change the bytes...
        assert_ne!(reformatted, text, "{}: reformat was a no-op", cfg.dir);
        assert_ne!(
            reformatted.len(),
            text.len(),
            "{}: reformat must change the byte count",
            cfg.dir
        );

        // ...and the fingerprint did not move.
        let parsed = dataset::parse(&cfg, &reformatted).unwrap();
        let computed = fingerprint::hex(fingerprint::fingerprint(&parsed.games));
        let manifest = load_manifest(&cfg);
        assert_eq!(
            computed, manifest.fingerprint,
            "{}: the fingerprint is textual, not semantic",
            cfg.dir
        );
    }
}

/// CRLF line endings, uppercase hex, three-space separators, a trailing space
/// on every line: the same games, different bytes.
fn reformat(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + text.len() / 2);
    for (i, line) in text.lines().enumerate() {
        if i == 0 {
            out.push_str(line);
            out.push_str("   ");
        } else {
            let fields: Vec<&str> = line.split(' ').collect();
            out.push_str(fields[0]);
            for word in &fields[1..11] {
                out.push_str("   ");
                out.push_str(&word.to_uppercase());
            }
            out.push_str("   ");
            out.push_str(fields[11]);
            out.push(' ');
        }
        out.push_str("\r\n");
    }
    out
}

// ------------------------------ T7 changing one game moves the fingerprint

#[test]
fn changing_one_game_moves_the_fingerprint() {
    for cfg in dataset::CONFIGS {
        let parsed = load(&cfg);
        let base = fingerprint::fingerprint(&parsed.games);

        let mut flipped_bit = parsed.games.clone();
        flipped_bit[512].board[3] ^= 1;
        assert_ne!(
            fingerprint::fingerprint(&flipped_bit),
            base,
            "{}: flipping one board bit did not move the fingerprint",
            cfg.dir
        );

        let mut changed_letter = parsed.games.clone();
        let at = changed_letter[512].sequence.len() / 2;
        let letter = changed_letter[512].sequence[at];
        changed_letter[512].sequence[at] = if letter == b'I' { b'O' } else { b'I' };
        assert_ne!(
            fingerprint::fingerprint(&changed_letter),
            base,
            "{}: changing one sequence letter did not move the fingerprint",
            cfg.dir
        );

        let mut changed_seed = parsed.games.clone();
        changed_seed[512].seed += 1;
        assert_ne!(
            fingerprint::fingerprint(&changed_seed),
            base,
            "{}: changing one seed did not move the fingerprint",
            cfg.dir
        );

        // The header is not hashed: a different header cannot move it. (This
        // header still parses -- same seed range, same pieces, so the letters
        // are the right length -- it is simply not the canonical line.)
        let header_changed = format!(
            "# tetris-lab dataset 9 goal=99 pieces={} cap={} seeds={}-{}\n{}",
            cfg.pieces,
            cfg.cap,
            dataset::SEED_FIRST,
            dataset::SEED_LAST,
            {
                let mut s = String::new();
                for game in &parsed.games {
                    s.push_str(&dataset::game_line(game));
                    s.push('\n');
                }
                s
            }
        );
        let reparsed = dataset::parse(&cfg, &header_changed).unwrap();
        assert_eq!(
            fingerprint::fingerprint(&reparsed.games),
            base,
            "{}: the fingerprint must not cover the header",
            cfg.dir
        );
    }
}

// ------------------------------------------------ T8 the cap, corrected (§5.7)

#[test]
fn every_board_respects_the_cap() {
    // §5.7 and ADR 0003 say every committed board's tallest column equals the
    // cap *exactly*. That is false and this test records the correction
    // (ticket 02, operator decision; the spec amendment is tickets 06/18):
    // the pre-fill loop breaks on the first draw whose resting top would exceed
    // the cap, and the crossing piece can overshoot by more than one row (a
    // pre-fill line clear lowers the stack further), so a board may be shorter
    // than the cap. Measured over seeds 0-1999 at cap 7: 1074/2000 reach exactly
    // 7, 926 are shorter, 0 exceed it. So: `tallest <= cap` per game, and
    // `max over the configuration == cap` -- never per-game equality.
    for cfg in dataset::CONFIGS {
        let parsed = load(&cfg);
        let mut tallest_over_set = 0;
        let mut at_cap = 0;
        for game in &parsed.games {
            let board: Board = game.board();
            let tallest = board.tallest();
            assert!(
                tallest <= cfg.cap,
                "{}: seed {} has a column of {} cells, above cap {}",
                cfg.dir,
                game.seed,
                tallest,
                cfg.cap
            );
            assert_eq!(
                board.full_rows(),
                0,
                "{}: seed {} left a completed row un-cleared",
                cfg.dir,
                game.seed
            );
            assert!(
                board.is_legal(),
                "{}: seed {} produced an illegal board (bits above row 0)",
                cfg.dir,
                game.seed
            );
            tallest_over_set = tallest_over_set.max(tallest);
            if tallest == cfg.cap {
                at_cap += 1;
            }
        }
        assert_eq!(
            tallest_over_set, cfg.cap,
            "{}: no committed game reaches the cap, so the configuration does not exercise it",
            cfg.dir
        );
        assert!(at_cap > 0, "{}: no board at the cap", cfg.dir);
    }
}

// ------------------------------------------- T9 the configurations pair on a seed

#[test]
fn the_configurations_pair_on_a_seed() {
    let p40 = load(&dataset::GOAL8);
    let p100 = load(&dataset::GOAL12);
    assert_eq!(p40.games.len(), p100.games.len());
    for (short, long) in p40.games.iter().zip(p100.games.iter()) {
        assert_eq!(short.seed, long.seed, "the files are not seed-aligned");
        assert_eq!(
            short.board, long.board,
            "seed {}: the same seed has two different boards",
            short.seed
        );
        assert_eq!(
            &long.sequence[..short.sequence.len()],
            &short.sequence[..],
            "seed {}: the 100-piece sequence does not nest the 40-piece one",
            short.seed
        );
    }
}

// ---------------------------------------------------- T10 no seed out of domain

#[test]
fn no_seed_is_out_of_domain() {
    for cfg in dataset::CONFIGS {
        for seed in dataset::SEED_FIRST..=dataset::SEED_LAST {
            engine::generate_game(seed, cfg.pieces, cfg.cap)
                .unwrap_or_else(|e| panic!("{}: seed {seed} is out of domain: {e:?}", cfg.dir));
        }
    }
}

// ----------------------------- T11 reference.jsonl exists, is empty, is untouched

#[test]
fn reference_jsonl_is_empty_and_untouched() {
    for cfg in dataset::CONFIGS {
        let path = dataset::reference_path(&dataset::root(), &cfg);
        assert!(path.exists(), "{}: reference.jsonl is missing", cfg.dir);
        assert_eq!(
            fs::metadata(&path).unwrap().len(),
            0,
            "{}: reference.jsonl must be empty until a Reference run lands",
            cfg.dir
        );
    }

    // A dry-run export must not truncate or rewrite it (§6: append-only).
    let root = scratch_root("reference");
    for cfg in dataset::CONFIGS {
        copy_config(&dataset::root(), &root, &cfg);
    }
    for cfg in dataset::CONFIGS {
        export::run(&cfg, &root, None, true).expect("dry-run export");
        let path = dataset::reference_path(&root, &cfg);
        assert_eq!(
            fs::metadata(&path).unwrap().len(),
            0,
            "{}: a dry-run export touched the append-only file",
            cfg.dir
        );
    }
    let _ = fs::remove_dir_all(&root);
}

// ----------------------------------------- T12 nothing under the Dataset is ignored

#[test]
fn nothing_under_the_dataset_is_ignored() {
    let root = dataset::root();
    let repo = root.parent().unwrap().parent().unwrap();

    for cfg in dataset::CONFIGS {
        for kind in dataset::Kind::ALL {
            let path = dataset::config_dir(&root, &cfg).join(kind.file());
            let status = Command::new("git")
                .arg("check-ignore")
                .arg("--no-index")
                .arg("-q")
                .arg(&path)
                .current_dir(repo)
                .status()
                .expect("git check-ignore");
            assert!(
                !status.success(),
                "{} is ignored by .gitignore; nothing under data/dataset/ may ever be ignored",
                path.display()
            );
            // And it is genuinely tracked, not merely unignored.
            let status = Command::new("git")
                .arg("ls-files")
                .arg("--error-unmatch")
                .arg(&path)
                .current_dir(repo)
                .output()
                .expect("git ls-files");
            assert!(
                status.status.success(),
                "{} is not tracked by git",
                path.display()
            );
        }
    }

    // The Dataset's bytes are pinned: `core.autocrlf=true` must not translate
    // them, or a byte comparison in the worktree is meaningless.
    let status = Command::new("git")
        .arg("check-attr")
        .arg("-z")
        .arg("text")
        .arg("--")
        .arg(root.join("goal8-p40-cap7/games.txt"))
        .current_dir(repo)
        .output()
        .expect("git check-attr");
    let text = String::from_utf8_lossy(&status.stdout);
    assert!(
        text.contains("text: unset") || text.contains("text: set") || text.contains("text: false"),
        "data/dataset/ is not pinned in .gitattributes: {text}"
    );
}

// ---------------------------------------- T13 archiving is never silent (§6.2)

#[test]
fn archiving_is_never_silent() {
    let root = scratch_root("archive");
    let cfg = dataset::GOAL8;
    for cfg in dataset::CONFIGS {
        copy_config(&dataset::root(), &root, &cfg);
    }

    // A copy that agrees with the engine exports to the same bytes and archives
    // nothing.
    let before = fs::read_to_string(dataset::games_path(&root, &cfg)).unwrap();
    let report = export::run(&cfg, &root, None, false).expect("idempotent export");
    assert_eq!(report.fingerprint, load_manifest(&cfg).fingerprint);
    assert_eq!(
        report.action,
        export::Action::Unchanged,
        "an idempotent export must not claim a change"
    );
    assert_eq!(
        fs::read_to_string(dataset::games_path(&root, &cfg)).unwrap(),
        before
    );
    assert!(
        !dataset::archive_dir(&root, &cfg).exists()
            || fs::read_dir(dataset::archive_dir(&root, &cfg))
                .unwrap()
                .next()
                .is_none(),
        "an unchanged export archived something"
    );

    // Now move one game in the file: the export must refuse to write.
    let path = dataset::games_path(&root, &cfg);
    let text = fs::read_to_string(&path).unwrap();
    let parsed = dataset::parse(&cfg, &text).unwrap();
    let moved = parsed.games[7].board[0] ^ 0x1;
    let mut lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
    let fields: Vec<&str> = lines[8].split(' ').collect();
    let mut rebuilt = String::new();
    for (i, field) in fields.iter().enumerate() {
        if i > 0 {
            rebuilt.push(' ');
        }
        if i == 1 {
            rebuilt.push_str(&format!("{moved:08x}"));
        } else {
            rebuilt.push_str(field);
        }
    }
    lines[8] = rebuilt;
    let edited = lines.join("\n") + "\n";
    fs::write(&path, &edited).unwrap();
    let old_fingerprint = fingerprint::hex(fingerprint::fingerprint(
        &dataset::parse(&cfg, &edited).unwrap().games,
    ));

    let drift = export::run(&cfg, &root, None, false)
        .expect_err("a moved game must make the exporter refuse");
    let message = drift.to_string();
    assert!(
        message.contains("--accept-change"),
        "the refusal tells you what to do: {message}"
    );
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        edited,
        "a refused export wrote anyway"
    );

    // With a reason, §6.2 happens mechanically.
    let manifest_before = load_manifest_at(&root, &cfg);
    let report = export::run(
        &cfg,
        &root,
        Some("ticket 02 test: moved one game on purpose"),
        false,
    )
    .expect("accepted change");
    assert!(matches!(report.action, export::Action::Accepted { .. }));

    let archived = dataset::archive_dir(&root, &cfg).join(format!("{old_fingerprint}.txt"));
    assert!(
        archived.exists(),
        "no archive file at {}",
        archived.display()
    );
    assert_eq!(
        fs::read_to_string(&archived).unwrap(),
        edited,
        "the archive must hold the file it replaced, byte for byte"
    );

    let manifest_after = load_manifest_at(&root, &cfg);
    assert_eq!(
        manifest_after.generator_version,
        manifest_before.generator_version + 1,
        "generator_version must bump exactly once"
    );
    assert_eq!(
        manifest_after.notes.len(),
        manifest_before.notes.len() + 1,
        "exactly one note line"
    );
    assert_eq!(
        manifest_after.notes.last().unwrap(),
        "ticket 02 test: moved one game on purpose"
    );
    assert_eq!(
        manifest_after.fingerprint,
        fingerprint::hex(fingerprint::fingerprint(
            &dataset::parse(&cfg, &fs::read_to_string(&path).unwrap())
                .unwrap()
                .games
        )),
        "the manifest must carry the new fingerprint"
    );

    // Re-exported, the Dataset is green again.
    verify::check_config(&cfg, &root).expect("the re-exported Dataset verifies");
    let _ = fs::remove_dir_all(&root);
}

// ------------------------------------------------- the gate, over both configs

#[test]
fn the_gate_passes_on_the_committed_dataset() {
    let reports = verify::run_all(&dataset::root()).expect("the committed Dataset is green");
    assert_eq!(reports.len(), dataset::CONFIGS.len());
    for report in &reports {
        assert_eq!(
            report.games.len(),
            dataset::GAME_COUNT,
            "{}",
            report.config.dir
        );
        assert_eq!(report.fingerprint, report.manifest.fingerprint);
    }
}

// ------------------------------------------------------------- corpus hygiene

#[test]
fn the_export_is_one_line_per_game() {
    for cfg in dataset::CONFIGS {
        let path = dataset::games_path(&dataset::root(), &cfg);
        let bytes = fs::read(&path).unwrap();
        assert!(bytes.is_ascii(), "{}: not ASCII", cfg.dir);
        assert!(
            !bytes.contains(&b'\r'),
            "{}: CR in the committed file",
            cfg.dir
        );
        assert_eq!(
            bytes.last(),
            Some(&b'\n'),
            "{}: no trailing newline",
            cfg.dir
        );
        let text = String::from_utf8(bytes).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), dataset::GAME_COUNT + 1, "{}", cfg.dir);
        for line in &lines[1..] {
            let fields: Vec<&str> = line.split(' ').collect();
            assert_eq!(fields.len(), 12, "{}: {line:?}", cfg.dir);
            assert!(
                !line.contains("  "),
                "{}: double space in {line:?}",
                cfg.dir
            );
            for word in &fields[1..11] {
                assert_eq!(word.len(), 8, "{}: {line:?}", cfg.dir);
                assert_eq!(
                    *word,
                    word.to_lowercase(),
                    "{}: hex must be lowercase: {line:?}",
                    cfg.dir
                );
            }
            assert_eq!(
                fields[11].len(),
                cfg.pieces as usize,
                "{}: sequence length: {line:?}",
                cfg.dir
            );
            assert!(
                fields[11].bytes().all(|b| b"IJLOSTZ".contains(&b)),
                "{}: sequence letters: {line:?}",
                cfg.dir
            );
        }
    }
}
