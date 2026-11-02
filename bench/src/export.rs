//! The exporter: generate both configurations from their seeds and write them
//! (§6), with §6.2's legitimate-change procedure turned into mechanics.
//!
//! It refuses to move a game silently. If the freshly generated games differ
//! from the committed file, nothing is written unless a one-line reason is
//! passed; then the old file is archived under its own fingerprint, the version
//! is bumped and the reason is recorded -- in the manifest, in that order, so an
//! interrupted run leaves either the old Dataset or a stray `.tmp` the gate
//! fails on, never half of each.
//!
//! `reference.jsonl` is created if it is absent and otherwise never opened for
//! write: it is append-only and a re-export must not destroy measurements.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::dataset::{self, Config, GameLine, Kind, Manifest};
use crate::fingerprint;
use crate::verify::Drift;

/// What the export actually did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// The configuration did not exist yet.
    Created,
    /// The games are the same as before. The bytes may have been rewritten
    /// (canonicalised); nothing was archived and no version moved.
    Unchanged,
    /// The games moved and the change was accepted with a reason.
    Accepted {
        /// `archive/<old-fingerprint>.txt`
        archive: PathBuf,
        /// The fingerprint the archived file holds.
        replaced: String,
    },
}

#[derive(Clone, Debug)]
pub struct ExportReport {
    pub config: Config,
    pub path: PathBuf,
    /// The fingerprint of the games just generated, 16 lowercase hex digits.
    pub fingerprint: String,
    pub bytes: usize,
    pub games: usize,
    pub action: Action,
    /// True when nothing was written (a dry run).
    pub dry_run: bool,
}

/// Generate one configuration, render it, and write it -- or refuse.
pub fn run(
    cfg: &Config,
    data: &Path,
    accept_change: Option<&str>,
    dry_run: bool,
) -> Result<ExportReport, Drift> {
    let dir = dataset::config_dir(data, cfg);
    let games_path = dataset::games_path(data, cfg);
    let manifest_path = dataset::manifest_path(data, cfg);

    // 1. Generate. The engine's own function: no second Generator, no solve.
    let mut games: Vec<GameLine> = Vec::with_capacity(dataset::GAME_COUNT);
    for seed in dataset::SEED_FIRST..=dataset::SEED_LAST {
        let game =
            engine::generate_game(seed, cfg.pieces, cfg.cap).map_err(|error| Drift::Domain {
                path: games_path.clone(),
                seed,
                message: format!("{error:?}"),
            })?;
        games.push(GameLine::from_engine(&game));
    }
    let text = dataset::render(cfg, &games);
    let new_fingerprint = fingerprint::fingerprint(&games);
    let new_hex = fingerprint::hex(new_fingerprint);

    // 2. Compare against what is committed, and decide.
    let mut generator_version = 1u32;
    let mut notes: Vec<String> = Vec::new();
    let mut action = Action::Created;

    if games_path.exists() {
        let committed = read(&games_path)?;
        let parsed = dataset::parse(cfg, &committed).map_err(|error| Drift::Parse {
            path: games_path.clone(),
            line: error.line,
            message: error.message,
        })?;
        let committed_fingerprint = fingerprint::fingerprint(&parsed.games);

        // An existing manifest keeps its history: the version and the notes are
        // not the exporter's to reset.
        if manifest_path.exists() {
            if let Ok(manifest) = Manifest::parse(&read(&manifest_path)?) {
                generator_version = manifest.generator_version;
                notes = manifest.notes;
            }
        }

        if committed_fingerprint == new_fingerprint {
            action = Action::Unchanged;
        } else {
            let differing = count_differing(&parsed.games, &games);
            let reason = accept_change
                .map(str::trim)
                .filter(|reason| !reason.is_empty());
            let Some(reason) = reason else {
                return Err(Drift::UnacceptedChange {
                    path: games_path,
                    committed: fingerprint::hex(committed_fingerprint),
                    regenerated: new_hex,
                    differing,
                    total: parsed.games.len(),
                });
            };
            let archive_dir = dataset::archive_dir(data, cfg);
            let archive =
                archive_dir.join(format!("{}.txt", fingerprint::hex(committed_fingerprint)));
            if !dry_run {
                fs::create_dir_all(&archive_dir).map_err(|error| Drift::Io {
                    path: archive_dir.clone(),
                    message: error.to_string(),
                })?;
                // Never overwrite an archive: §6.2 keeps every one forever.
                if !archive.exists() {
                    fs::copy(&games_path, &archive).map_err(|error| Drift::Io {
                        path: archive.clone(),
                        message: error.to_string(),
                    })?;
                }
            }
            generator_version += 1;
            notes.push(reason.to_string());
            action = Action::Accepted {
                archive,
                replaced: fingerprint::hex(committed_fingerprint),
            };
        }
    }

    let manifest = Manifest {
        generator_version,
        notes,
        fingerprint: new_hex.clone(),
        ..Manifest::for_config(cfg, &new_hex)
    };

    let report = ExportReport {
        config: *cfg,
        path: games_path.clone(),
        fingerprint: new_hex,
        bytes: text.len(),
        games: games.len(),
        action,
        dry_run,
    };
    if dry_run {
        return Ok(report);
    }

    // 3. Write, staging first so a crash cannot leave a truncated Dataset.
    fs::create_dir_all(&dir).map_err(|error| Drift::Io {
        path: dir.clone(),
        message: error.to_string(),
    })?;
    write_atomic(&games_path, text.as_bytes())?;
    write_atomic(&manifest_path, manifest.render().as_bytes())?;

    // 4. `reference.jsonl` exists and is empty; an existing one is untouchable.
    let reference = dataset::reference_path(data, cfg);
    if !reference.exists() {
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&reference)
            .map_err(|error| Drift::Io {
                path: reference.clone(),
                message: error.to_string(),
            })?;
    }

    Ok(report)
}

fn read(path: &Path) -> Result<String, Drift> {
    fs::read_to_string(path).map_err(|error| Drift::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

fn count_differing(committed: &[GameLine], fresh: &[GameLine]) -> usize {
    let mut differing = 0;
    for index in 0..committed.len().max(fresh.len()) {
        match (committed.get(index), fresh.get(index)) {
            (Some(a), Some(b)) if a == b => {}
            _ => differing += 1,
        }
    }
    differing
}

/// Write through `<file>.tmp` in the same directory, then rename over the
/// target: a reader sees the old file or the new one.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), Drift> {
    let staging = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&staging).map_err(|error| Drift::Io {
            path: staging.clone(),
            message: error.to_string(),
        })?;
        file.write_all(bytes).map_err(|error| Drift::Io {
            path: staging.clone(),
            message: error.to_string(),
        })?;
        file.sync_all().map_err(|error| Drift::Io {
            path: staging.clone(),
            message: error.to_string(),
        })?;
    }
    fs::rename(&staging, path).map_err(|error| Drift::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    Ok(())
}

/// Every committed path of a configuration, for the CLI's summary line.
pub fn committed_files(cfg: &Config, data: &Path) -> Vec<PathBuf> {
    Kind::ALL
        .iter()
        .map(|kind| dataset::path(data, cfg, *kind))
        .collect()
}
