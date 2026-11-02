//! The gate: §9.2 gate 1 and gate 2 over one configuration (§6), plus the
//! properties the Dataset is supposed to have.
//!
//! It regenerates every committed game from its Seed in memory, compares every
//! Board word and every Sequence letter to the committed file, and recomputes
//! the fingerprint from *both* the file and the regenerated games. The failure
//! names the first game that moved -- seed, position, line, field, expected and
//! actual -- and counts the rest, so a wholesale move reads as "2000 of 2000"
//! rather than as one seed.

use std::fs;
use std::path::{Path, PathBuf};

use crate::dataset::{self, Config, GameLine, Manifest};
use crate::fingerprint;

/// One configuration, verified.
#[derive(Clone, Debug)]
pub struct ConfigReport {
    pub config: Config,
    pub path: PathBuf,
    pub manifest: Manifest,
    /// The games as the committed file has them.
    pub games: Vec<GameLine>,
    /// The fingerprint, 16 lowercase hex digits; equal to `manifest.fingerprint`.
    pub fingerprint: String,
    /// The tallest column over the whole set: it must equal the cap (§5.7, as
    /// corrected in ticket 02).
    pub tallest_over_set: u32,
    /// How many games reach the cap exactly. Useful in the gate's output, never
    /// asserted per game.
    pub at_cap: usize,
}

/// The first game whose bits moved, and how many others did too. Boxed inside
/// [`Drift`] because it carries enough text to name a game precisely, and a
/// drift error is returned once, never in a loop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameDrift {
    pub path: PathBuf,
    /// 1-based position in seed order.
    pub index: usize,
    /// 1-based line number, header included.
    pub line: usize,
    pub seed: u32,
    pub field: String,
    /// What the engine makes now.
    pub expected: String,
    /// What the committed file says.
    pub actual: String,
    pub differing: usize,
    pub total: usize,
    pub manifest_fingerprint: String,
    pub file_fingerprint: String,
}

/// Why the Dataset is red. `Display` is the message a contributor reads, and it
/// always names the file, the position and the two values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Drift {
    Io {
        path: PathBuf,
        message: String,
    },
    Parse {
        path: PathBuf,
        line: usize,
        message: String,
    },
    Header {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    Manifest {
        path: PathBuf,
        field: String,
        expected: String,
        actual: String,
    },
    /// A missing committed file.
    Missing {
        path: PathBuf,
    },
    /// A staging file left behind by an interrupted export.
    Stray {
        path: PathBuf,
    },
    /// The generator refused a seed that the Dataset claims to hold.
    Domain {
        path: PathBuf,
        seed: u32,
        message: String,
    },
    /// The first game whose bits moved, and how many others did too.
    Game(Box<GameDrift>),
    Fingerprint {
        path: PathBuf,
        manifest_fingerprint: String,
        file_fingerprint: String,
        what: &'static str,
    },
    /// A board taller than the cap: a crossing piece was placed.
    Cap {
        path: PathBuf,
        seed: u32,
        tallest: u32,
        cap: u32,
    },
    /// No board in the configuration reaches the cap, so it does not exercise it.
    CapNotReached {
        path: PathBuf,
        tallest: u32,
        cap: u32,
    },
    /// The same seed is two different games in the two configurations (§5.7).
    Pairing {
        seed: u32,
        field: String,
        expected: String,
        actual: String,
    },
    /// The exporter refused to move a game without a reason (§6.2).
    UnacceptedChange {
        path: PathBuf,
        committed: String,
        regenerated: String,
        differing: usize,
        total: usize,
    },
}

const HINT: &str = "  hint: the Generator moved, or the file was edited. If the generator change\n        is intended, re-export with --accept-change \"<one line reason>\"\n        (see data/dataset/README.md). If you did not change the generator, the\n        file is wrong.";

impl std::fmt::Display for Drift {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Drift::Io { path, message } => {
                write!(f, "error: cannot read {}\n  {message}", path.display())
            }
            Drift::Parse {
                path,
                line,
                message,
            } => write!(
                f,
                "error: dataset drift in {}\n  first mismatch: line {line}, which does not parse\n    {message}\n{HINT}",
                path.display()
            ),
            Drift::Header {
                path,
                expected,
                actual,
            } => write!(
                f,
                "error: dataset drift in {}\n  first mismatch: line 1, the header\n    field:    header line\n    expected: {expected}\n    actual:   {actual}\n{HINT}",
                path.display()
            ),
            Drift::Manifest {
                path,
                field,
                expected,
                actual,
            } => write!(
                f,
                "error: dataset drift in {}\n    field:    manifest field {field}\n    expected: {expected}\n    actual:   {actual}\n{HINT}",
                path.display()
            ),
            Drift::Missing { path } => write!(
                f,
                "error: {} is missing from the Dataset\n{HINT}",
                path.display()
            ),
            Drift::Stray { path } => write!(
                f,
                "error: stray staging file {}\n  An export was interrupted. Re-run it; the target file is\n  either the old one or the new one, never half of each.",
                path.display()
            ),
            Drift::Domain {
                path,
                seed,
                message,
            } => write!(
                f,
                "error: dataset drift in {}\n    seed:     {seed} is out of the generator's domain ({message})\n  A committed configuration may not hold a game the engine refuses.",
                path.display()
            ),
            Drift::Game(game) => {
                let GameDrift {
                    path,
                    index,
                    line,
                    seed,
                    field,
                    expected,
                    actual,
                    differing,
                    total,
                    manifest_fingerprint,
                    file_fingerprint,
                } = game.as_ref();
                write!(
                    f,
                    "error: dataset drift in {}
  first mismatch: game {index} of {total}, line {line}, seed {seed}
    field:    {field}
    expected: {expected}
    actual:   {actual}
  fingerprint: manifest {manifest_fingerprint}
               file     {file_fingerprint}
  games that differ: {differing} of {total}
{HINT}",
                    path.display()
                )
            }
            Drift::Fingerprint {
                path,
                manifest_fingerprint,
                file_fingerprint,
                what,
            } => write!(
                f,
                "error: dataset drift in {}\n    field:    fingerprint of the {what}\n    expected: {manifest_fingerprint} (manifest)\n    actual:   {file_fingerprint}\n{HINT}",
                path.display()
            ),
            Drift::Cap {
                path,
                seed,
                tallest,
                cap,
            } => write!(
                f,
                "error: dataset drift in {}\n    seed:     {seed}\n    field:    tallest column\n    expected: <= {cap}\n    actual:   {tallest}\n  A crossing piece was placed, or the cap was written wrong.\n{HINT}",
                path.display()
            ),
            Drift::CapNotReached { path, tallest, cap } => write!(
                f,
                "error: dataset drift in {}\n    field:    tallest column over the whole configuration\n    expected: == {cap} (some game must reach the cap)\n    actual:   {tallest}\n{HINT}",
                path.display()
            ),
            Drift::Pairing {
                seed,
                field,
                expected,
                actual,
            } => write!(
                f,
                "error: dataset drift between the two configurations\n    seed:     {seed}\n    field:    {field}\n    expected: {expected} (goal8-p40-cap7)\n    actual:   {actual} (goal12-p100-cap7)\n  Both configurations use cap 7, so the same seed must have the identical\n  Board and a nested Sequence (§5.7).",
            ),
            Drift::UnacceptedChange {
                path,
                committed,
                regenerated,
                differing,
                total,
            } => write!(
                f,
                "error: the Generator no longer makes {}\n  fingerprint: committed   {committed}\n               regenerated {regenerated}\n  games that differ: {differing} of {total}\n  the exporter wrote nothing.\n  hint: if the change is intended, re-run with\n        --accept-change \"<one line reason>\" (see data/dataset/README.md).\n        That archives the committed file under its fingerprint, bumps\n        generator_version and records the reason (§6.2).",
                path.display()
            ),
        }
    }
}

fn read(path: &Path) -> Result<String, Drift> {
    fs::read_to_string(path).map_err(|error| Drift::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

fn manifest_field(
    path: &Path,
    field: &str,
    expected: impl ToString,
    actual: impl ToString,
) -> Drift {
    Drift::Manifest {
        path: path.to_path_buf(),
        field: field.to_string(),
        expected: expected.to_string(),
        actual: actual.to_string(),
    }
}

/// Verify one configuration. The same function the CI gate calls, and the same
/// one the tests call.
pub fn check_config(cfg: &Config, data: &Path) -> Result<ConfigReport, Drift> {
    let dir = dataset::config_dir(data, cfg);
    let games_path = dataset::games_path(data, cfg);
    let manifest_path = dataset::manifest_path(data, cfg);

    if !games_path.exists() {
        return Err(Drift::Missing { path: games_path });
    }
    let text = read(&games_path)?;
    let parsed = dataset::parse(cfg, &text).map_err(|error| Drift::Parse {
        path: games_path.clone(),
        line: error.line,
        message: error.message,
    })?;

    // §9.2 gate 2, part one: the header is the canonical line, not merely a
    // line with the right fields in it.
    let canonical = dataset::header_line(cfg);
    if parsed.header_line != canonical {
        return Err(Drift::Header {
            path: games_path.clone(),
            expected: canonical,
            actual: parsed.header_line.clone(),
        });
    }

    // An interrupted export leaves a staging file; fail on it rather than ship a
    // half-written Dataset.
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().ends_with(".tmp") {
                return Err(Drift::Stray { path: entry.path() });
            }
        }
    }

    // §9.2 gate 2, part two: the manifest agrees with the file it describes.
    if !manifest_path.exists() {
        return Err(Drift::Missing {
            path: manifest_path,
        });
    }
    let manifest_text = read(&manifest_path)?;
    let manifest = Manifest::parse(&manifest_text).map_err(|message| Drift::Manifest {
        path: manifest_path.clone(),
        field: "manifest.json".to_string(),
        expected: "a valid manifest (§6)".to_string(),
        actual: message,
    })?;

    if manifest.format != parsed.header.format {
        return Err(manifest_field(
            &manifest_path,
            "format",
            parsed.header.format,
            manifest.format,
        ));
    }
    if manifest.goal != parsed.header.goal {
        return Err(manifest_field(
            &manifest_path,
            "goal",
            parsed.header.goal,
            manifest.goal,
        ));
    }
    if manifest.goal != cfg.goal {
        return Err(manifest_field(
            &manifest_path,
            "goal",
            cfg.goal,
            manifest.goal,
        ));
    }
    if manifest.pieces != parsed.header.pieces {
        return Err(manifest_field(
            &manifest_path,
            "pieces",
            parsed.header.pieces,
            manifest.pieces,
        ));
    }
    if manifest.pieces != cfg.pieces {
        return Err(manifest_field(
            &manifest_path,
            "pieces",
            cfg.pieces,
            manifest.pieces,
        ));
    }
    if manifest.cap != parsed.header.cap {
        return Err(manifest_field(
            &manifest_path,
            "cap",
            parsed.header.cap,
            manifest.cap,
        ));
    }
    if manifest.cap != cfg.cap {
        return Err(manifest_field(&manifest_path, "cap", cfg.cap, manifest.cap));
    }
    if manifest.test_split != dataset::TEST_SPLIT {
        return Err(manifest_field(
            &manifest_path,
            "splits.test",
            format!("{:?}", dataset::TEST_SPLIT),
            format!("{:?}", manifest.test_split),
        ));
    }
    if manifest.tuning_split != dataset::TUNING_SPLIT {
        return Err(manifest_field(
            &manifest_path,
            "splits.tuning",
            format!("{:?}", dataset::TUNING_SPLIT),
            format!("{:?}", manifest.tuning_split),
        ));
    }
    if manifest.seeds != (parsed.header.first_seed, parsed.header.last_seed) {
        return Err(manifest_field(
            &manifest_path,
            "seeds",
            format!("{:?}", (parsed.header.first_seed, parsed.header.last_seed)),
            format!("{:?}", manifest.seeds),
        ));
    }
    let first_seed = parsed.games.first().map(|game| game.seed);
    if first_seed != Some(manifest.seeds.0) {
        return Err(manifest_field(
            &manifest_path,
            "seeds.first",
            manifest.seeds.0,
            format!("{first_seed:?}"),
        ));
    }
    if manifest.game_count as usize != parsed.games.len() {
        return Err(manifest_field(
            &manifest_path,
            "game_count",
            parsed.games.len(),
            manifest.game_count,
        ));
    }
    if manifest.generator_version == 0 {
        return Err(manifest_field(
            &manifest_path,
            "generator_version",
            ">= 1",
            0,
        ));
    }
    if manifest.fingerprint.len() != 16 || fingerprint::from_hex(&manifest.fingerprint).is_none() {
        return Err(manifest_field(
            &manifest_path,
            "fingerprint",
            "16 lowercase hex digits",
            &manifest.fingerprint,
        ));
    }

    // §9.2 gate 1: regenerate every game and compare the bits, so the failure
    // names *which* game moved rather than only that the hash differs.
    let file_fingerprint = fingerprint::hex(fingerprint::fingerprint(&parsed.games));
    let mut regenerated: Vec<GameLine> = Vec::with_capacity(parsed.games.len());
    let mut first_mismatch: Option<Drift> = None;
    let mut differing = 0usize;
    let total = parsed.games.len();
    let mut tallest_over_set = 0u32;
    let mut at_cap = 0usize;
    let mut cap_drift: Option<Drift> = None;

    for (offset, committed) in parsed.games.iter().enumerate() {
        let game = engine::generate_game(committed.seed, cfg.pieces, cfg.cap).map_err(|error| {
            Drift::Domain {
                path: games_path.clone(),
                seed: committed.seed,
                message: format!("{error:?}"),
            }
        })?;
        let fresh = GameLine::from_engine(&game);

        if let Some((field, expected, actual)) = first_difference(committed, &fresh) {
            differing += 1;
            if first_mismatch.is_none() {
                first_mismatch = Some(Drift::Game(Box::new(GameDrift {
                    path: games_path.clone(),
                    index: offset + 1,
                    line: offset + 2,
                    seed: committed.seed,
                    field,
                    expected,
                    actual,
                    differing: 0, // filled in below, once the count is final
                    total,
                    manifest_fingerprint: manifest.fingerprint.clone(),
                    file_fingerprint: file_fingerprint.clone(),
                })));
            }
        }

        // §5.7 as corrected by ticket 02: no column is ever taller than the cap
        // (the crossing piece is never placed), but an individual board need not
        // reach it -- the crossing draw can overshoot by more than one row, and a
        // pre-fill line clear lowers the stack further. The set as a whole must
        // reach the cap; that is checked after the loop.
        let board = committed.board();
        let tallest = board.tallest();
        tallest_over_set = tallest_over_set.max(tallest);
        if tallest == cfg.cap {
            at_cap += 1;
        }
        if tallest > cfg.cap && cap_drift.is_none() && first_mismatch.is_none() {
            cap_drift = Some(Drift::Cap {
                path: games_path.clone(),
                seed: committed.seed,
                tallest,
                cap: cfg.cap,
            });
        }

        regenerated.push(fresh);
    }

    if let Some(mut drift) = first_mismatch {
        if let Drift::Game(ref mut game) = drift {
            game.differing = differing;
        }
        return Err(drift);
    }
    if let Some(drift) = cap_drift {
        return Err(drift);
    }

    // Both halves of "the bits and the fingerprint".
    if file_fingerprint != manifest.fingerprint {
        return Err(Drift::Fingerprint {
            path: games_path.clone(),
            manifest_fingerprint: manifest.fingerprint.clone(),
            file_fingerprint,
            what: "committed file",
        });
    }
    let regenerated_fingerprint = fingerprint::hex(fingerprint::fingerprint(&regenerated));
    if regenerated_fingerprint != manifest.fingerprint {
        return Err(Drift::Fingerprint {
            path: games_path.clone(),
            manifest_fingerprint: manifest.fingerprint.clone(),
            file_fingerprint: regenerated_fingerprint,
            what: "regenerated games",
        });
    }
    if tallest_over_set != cfg.cap {
        return Err(Drift::CapNotReached {
            path: games_path.clone(),
            tallest: tallest_over_set,
            cap: cfg.cap,
        });
    }

    if !dataset::reference_path(data, cfg).exists() {
        return Err(Drift::Missing {
            path: dataset::reference_path(data, cfg),
        });
    }

    Ok(ConfigReport {
        config: *cfg,
        path: games_path,
        manifest,
        games: parsed.games,
        fingerprint: file_fingerprint,
        tallest_over_set,
        at_cap,
    })
}

/// The first field that differs between the committed line and the regenerated
/// one, in the file's own vocabulary, with the expected (engine) and actual
/// (file) values in the file's canonical form.
fn first_difference(committed: &GameLine, fresh: &GameLine) -> Option<(String, String, String)> {
    if committed.seed != fresh.seed {
        return Some((
            "seed".to_string(),
            fresh.seed.to_string(),
            committed.seed.to_string(),
        ));
    }
    for column in 0..10 {
        if committed.board[column] != fresh.board[column] {
            return Some((
                format!("column word {column} (bit n = row 19-n)"),
                format!("{:08x}", fresh.board[column]),
                format!("{:08x}", committed.board[column]),
            ));
        }
    }
    if committed.sequence.len() != fresh.sequence.len() {
        return Some((
            "sequence length".to_string(),
            fresh.sequence.len().to_string(),
            committed.sequence.len().to_string(),
        ));
    }
    for (index, (committed_letter, fresh_letter)) in committed
        .sequence
        .iter()
        .zip(fresh.sequence.iter())
        .enumerate()
    {
        if committed_letter != fresh_letter {
            return Some((
                format!("sequence letter {index}"),
                (*fresh_letter as char).to_string(),
                (*committed_letter as char).to_string(),
            ));
        }
    }
    None
}

/// Verify every configuration, then the property that ties them together: one
/// seed, one Board, a nested Sequence (§5.7).
pub fn run_all(data: &Path) -> Result<Vec<ConfigReport>, Drift> {
    let mut reports = Vec::with_capacity(dataset::CONFIGS.len());
    for cfg in dataset::CONFIGS {
        reports.push(check_config(&cfg, data)?);
    }

    if reports.len() == 2 {
        let short = &reports[0].games;
        let long = &reports[1].games;
        if short.len() != long.len() {
            return Err(Drift::Pairing {
                seed: 0,
                field: "game_count".to_string(),
                expected: short.len().to_string(),
                actual: long.len().to_string(),
            });
        }
        for (short_game, long_game) in short.iter().zip(long.iter()) {
            if short_game.seed != long_game.seed {
                return Err(Drift::Pairing {
                    seed: short_game.seed,
                    field: "seed order".to_string(),
                    expected: short_game.seed.to_string(),
                    actual: long_game.seed.to_string(),
                });
            }
            if short_game.board != long_game.board {
                return Err(Drift::Pairing {
                    seed: short_game.seed,
                    field: "board".to_string(),
                    expected: short_game
                        .board
                        .iter()
                        .map(|word| format!("{word:08x}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                    actual: long_game
                        .board
                        .iter()
                        .map(|word| format!("{word:08x}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                });
            }
            if long_game.sequence[..short_game.sequence.len()] != short_game.sequence[..] {
                return Err(Drift::Pairing {
                    seed: short_game.seed,
                    field: "sequence (the 100-piece game must nest the 40-piece one)".to_string(),
                    expected: String::from_utf8_lossy(&short_game.sequence).to_string(),
                    actual: String::from_utf8_lossy(
                        &long_game.sequence[..short_game.sequence.len()],
                    )
                    .to_string(),
                });
            }
        }
    }

    Ok(reports)
}
