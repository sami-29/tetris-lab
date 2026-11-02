//! `bench` -- the native Dataset tool (§6, §9.2 gate 1).
//!
//! ```text
//! bench export [--config <name|all>] [--data <dir>] [--accept-change "<reason>"] [--dry-run]
//! bench verify [--config <name|all>] [--data <dir>] [--json]
//! bench fingerprint [--config <name|all>] [--data <dir>]
//! bench show [--config <name>] --seed <n> [--data <dir>]
//! bench gate <1|2|3|4|5|6|all> [--data <dir>] [--no-colour]
//! bench solve --emit [--config <name|all>] [--seeds <a,b,c>] [--beam <n>] [--data <dir>]
//! bench width-curve [--widths <w,w,...>] [--trials <n>] [--seeds-per-trial <n>]
//!                   [--quality-games <n>] [--ms native|wasm] [--out <file>]
//!                   [--reason "<one line>"] [--machine "<label>"]
//!                   [--overshoot <width>=<p90_ms> --overshoot-samples <n>
//!                    [--overshoot-substrate native|wasm] [--overshoot-note "<one line>"]]
//! bench width-curve --summarise <artifact> [--overshoot ...] [--out <file>]
//! bench tune [--seed <u64|0x...>] [--bed <name>] [--candidates <n>] [--games <n>]
//!            [--iterations <n>] [--data <dir>] [--out <file>] [--rows <file>]
//! bench tune --replay [<record>] [--data <dir>]
//! ```
//!
//! Exit codes: `0` clean, `1` the Dataset drifted or a Gate failed, `2` a usage
//! or I/O error or a Gate whose prerequisite is missing. Argument parsing is here
//! and nothing else is: every subcommand is one call into the library, which is
//! what the tests call too.

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bench::dataset::{self, Config};
use bench::gate::{self, Options as GateOptions};
use bench::tuning;
use bench::width::{self, MsSource, Options as WidthOptions, WIDTHS};
use bench::{export, fingerprint, show, verify};

const USAGE: &str = "\
usage:
  bench export      [--config <name|all>] [--data <dir>] [--accept-change \"<reason>\"] [--dry-run]
  bench verify      [--config <name|all>] [--data <dir>] [--json]
  bench fingerprint [--config <name|all>] [--data <dir>]
  bench show        [--config <name>] --seed <n> [--data <dir>]
  bench gate        <1|2|3|4|5|6|all> [--data <dir>] [--no-colour]
  bench solve       --emit [--config <name|all>] [--seeds <a,b,c|a-b>] [--beam <n>]
  bench width-curve [--widths <w,w,...>] [--trials <n>] [--seeds-per-trial <n>]
                    [--quality-games <n>] [--ms native|wasm] [--out <file>]
                    [--reason \"<one line>\"] [--machine \"<label>\"]
                    [--overshoot <width>=<p90_ms> --overshoot-samples <n>
                     [--overshoot-substrate native|wasm] [--overshoot-note \"<one line>\"]]
  bench width-curve --summarise <artifact> [--overshoot ...] [--out <file>]
  bench tune        [--seed <u64|0x...>] [--bed <name>] [--candidates <n>] [--games <n>]
                    [--iterations <n>] [--data <dir>] [--out <file>] [--rows <file>]
  bench tune        --replay [<record>] [--data <dir>]

configurations: goal8-p40-cap7, goal12-p100-cap7
width-curve defaults: every doubling point, 3 trials of 200 seeds, 1,000 quality
games, wasm milliseconds, written to data/measurements/width-curve.json.
--data defaults to the repository's data/dataset.
width-curve --summarise re-derives an artifact's summary fields (cap_bound,
complete, missing_widths, incomplete_reason) from the points and the probe it
already records, without re-measuring: it refuses to write if any measured number
would move, and it writes back to the artifact it was given unless --out is set.
--overshoot records a width probed outside the curve that failed the default
configuration's 200 ms p90 budget, which is what makes cap_bound \"budget\". A
probe is never a point: it has no place in the cost or quality series.
bench tune runs ticket 07's cross-entropy search over the tuning Split only, at
DEFAULT_BEAM read from data/measurements/width-curve.json, measures E0/E1/BCTS/E2
on all 1,000 tuning seeds and appends one run to data/measurements/e2-tuning.json
plus its per-seed rows. --replay re-runs a committed run's seed and checks the
table fingerprint; a rejected run exits 1.
exit codes: 0 clean, 1 Dataset drift, a failed Gate or a rejected tuning run,
2 usage, I/O, or a Gate whose prerequisite is missing.";

struct Args {
    configs: Vec<Config>,
    data: PathBuf,
    accept_change: Option<String>,
    dry_run: bool,
    json: bool,
    seed: Option<u32>,
    widths: Vec<u32>,
    trials: u32,
    seeds_per_trial: u32,
    quality_games: u32,
    ms: MsSource,
    /// `None` means "wherever this command's output belongs": the measurements
    /// path for a fresh curve, the artifact being re-derived for `--summarise`.
    out: Option<PathBuf>,
    reason: Option<String>,
    machine: Option<String>,
    /// `--summarise`: re-derive an artifact's summary fields instead of measuring.
    summarise: Option<PathBuf>,
    /// `--overshoot <width>=<p90_ms>`: a probed width and the p90 it failed on.
    overshoot: Option<(u32, f64)>,
    overshoot_samples: Option<u32>,
    overshoot_substrate: Option<MsSource>,
    overshoot_note: Option<String>,
}

impl Args {
    fn defaults() -> Args {
        Args {
            configs: dataset::CONFIGS.to_vec(),
            data: dataset::root(),
            accept_change: None,
            dry_run: false,
            json: false,
            seed: None,
            widths: WIDTHS.to_vec(),
            trials: 3,
            seeds_per_trial: 200,
            quality_games: 1_000,
            ms: MsSource::Wasm,
            out: None,
            reason: None,
            machine: None,
            summarise: None,
            overshoot: None,
            overshoot_samples: None,
            overshoot_substrate: None,
            overshoot_note: None,
        }
    }
}

/// `data/measurements/width-curve.json`, next to the Dataset.
fn measurements_path() -> PathBuf {
    dataset::root()
        .parent()
        .expect("data/dataset has a parent: data/")
        .join("measurements")
        .join("width-curve.json")
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = argv.first().cloned() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    if command == "--help" || command == "-h" || command == "help" {
        println!("{USAGE}");
        return ExitCode::from(0);
    }

    // The two subcommands with their own flags are dispatched before the Dataset
    // parser below, so neither can grow the other's argument surface.
    if command == "gate" {
        return ExitCode::from(gate_command(&argv[1..]));
    }
    if command == "solve" {
        return ExitCode::from(solve_command(&argv[1..]));
    }
    if command == "tune" {
        return ExitCode::from(tune_command(&argv[1..]));
    }

    let args = match parse(&argv[1..]) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("error: {message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };

    let outcome = match command.as_str() {
        "export" => export(&args),
        "verify" => verify(&args),
        "fingerprint" => fingerprint(&args),
        "show" => show_game(&args),
        "width-curve" => width_curve(&args),
        other => {
            eprintln!("error: unknown command {other:?}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match outcome {
        Ok(()) => ExitCode::from(0),
        Err(code) => ExitCode::from(code),
    }
}

fn parse(argv: &[String]) -> Result<Args, String> {
    let mut args = Args::defaults();
    let mut selected: Option<Vec<Config>> = None;
    let mut index = 0;
    while index < argv.len() {
        let flag = argv[index].as_str();
        let mut take = |what: &str| -> Result<String, String> {
            index += 1;
            argv.get(index)
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value ({what})"))
        };
        match flag {
            "--config" => {
                let value = take("a configuration name or all")?;
                if value == "all" {
                    selected = Some(dataset::CONFIGS.to_vec());
                } else {
                    let cfg = Config::from_name(&value).ok_or_else(|| {
                        format!(
                            "unknown configuration {value:?}; known: {}",
                            dataset::CONFIGS
                                .iter()
                                .map(|cfg| cfg.dir)
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    })?;
                    selected.get_or_insert_with(Vec::new).push(cfg);
                }
            }
            "--data" => args.data = PathBuf::from(take("a directory")?),
            "--accept-change" => args.accept_change = Some(take("a one-line reason")?),
            "--dry-run" => args.dry_run = true,
            "--json" => args.json = true,
            "--seed" => {
                let value = take("a seed number")?;
                args.seed = Some(
                    value
                        .parse::<u32>()
                        .map_err(|_| format!("--seed must be a number, found {value:?}"))?,
                );
            }
            "--widths" => {
                let value = take("a comma-separated list of widths")?;
                args.widths = value
                    .split(',')
                    .map(|field| {
                        field
                            .trim()
                            .parse::<u32>()
                            .map_err(|_| format!("--widths: {field:?} is not a width"))
                    })
                    .collect::<Result<Vec<u32>, String>>()?;
            }
            "--trials" => {
                let value = take("a repeat count")?;
                args.trials = value
                    .parse()
                    .map_err(|_| format!("--trials must be a number, found {value:?}"))?;
            }
            "--seeds-per-trial" => {
                let value = take("a seed count")?;
                args.seeds_per_trial = value
                    .parse()
                    .map_err(|_| format!("--seeds-per-trial must be a number, found {value:?}"))?;
            }
            "--quality-games" => {
                let value = take("a game count")?;
                args.quality_games = value
                    .parse()
                    .map_err(|_| format!("--quality-games must be a number, found {value:?}"))?;
            }
            "--ms" => {
                let value = take("native or wasm")?;
                args.ms = match value.as_str() {
                    "native" => MsSource::Native,
                    "wasm" => MsSource::Wasm,
                    other => return Err(format!("--ms must be native or wasm, found {other:?}")),
                };
            }
            "--out" => args.out = Some(PathBuf::from(take("a file")?)),
            "--reason" => args.reason = Some(take("a one-line reason")?),
            "--machine" => args.machine = Some(take("a machine label")?),
            "--summarise" => {
                args.summarise = Some(PathBuf::from(take("a width-curve artifact")?));
            }
            "--overshoot" => {
                let value = take("a probed width and its p90, like 1600=203.46")?;
                let (width, ms) = value.split_once('=').ok_or_else(|| {
                    format!("--overshoot wants <width>=<p90_ms>, found {value:?}")
                })?;
                let width = width
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| format!("--overshoot: {width:?} is not a width"))?;
                let ms = ms
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| format!("--overshoot: {ms:?} is not a p90 in ms"))?;
                args.overshoot = Some((width, ms));
            }
            "--overshoot-samples" => {
                let value = take("a seed count")?;
                args.overshoot_samples = Some(value.parse().map_err(|_| {
                    format!("--overshoot-samples must be a number, found {value:?}")
                })?);
            }
            "--overshoot-substrate" => {
                let value = take("native or wasm")?;
                args.overshoot_substrate = Some(match value.as_str() {
                    "native" => MsSource::Native,
                    "wasm" => MsSource::Wasm,
                    other => {
                        return Err(format!(
                            "--overshoot-substrate must be native or wasm, found {other:?}"
                        ))
                    }
                });
            }
            "--overshoot-note" => args.overshoot_note = Some(take("a one-line note")?),
            other => return Err(format!("unknown argument {other:?}")),
        }
        index += 1;
    }
    if let Some(selected) = selected {
        args.configs = selected;
    }
    Ok(args)
}

fn export(args: &Args) -> Result<(), u8> {
    let mut code = 0u8;
    for cfg in &args.configs {
        match export::run(cfg, &args.data, args.accept_change.as_deref(), args.dry_run) {
            Ok(report) => {
                let action = match &report.action {
                    export::Action::Created => "created".to_string(),
                    export::Action::Unchanged => "unchanged".to_string(),
                    export::Action::Accepted { archive, replaced } => {
                        format!("accepted, archived {replaced} -> {}", archive.display())
                    }
                };
                println!(
                    "{:<18} {:<10} {} {} games {} bytes{}  {}",
                    report.config.dir,
                    action,
                    report.fingerprint,
                    report.games,
                    report.bytes,
                    if report.dry_run {
                        "  (dry run, nothing written)"
                    } else {
                        ""
                    },
                    report.path.display()
                );
            }
            Err(drift) => {
                eprintln!("{drift}");
                code = 1;
            }
        }
    }
    if code == 0 {
        Ok(())
    } else {
        Err(code)
    }
}

fn verify(args: &Args) -> Result<(), u8> {
    let single = args.configs.len() == 1;
    let mut code = 0u8;
    for cfg in &args.configs {
        match verify::check_config(cfg, &args.data) {
            Ok(report) => {
                if args.json {
                    println!(
                        "{{\"config\":\"{}\",\"ok\":true,\"fingerprint\":\"{}\",\"games\":{},\"tallest\":{},\"at_cap\":{}}}",
                        report.config.dir,
                        report.fingerprint,
                        report.games.len(),
                        report.tallest_over_set,
                        report.at_cap
                    );
                } else {
                    println!(
                        "{:<18} ok  {}  {} games  cap {}: {} at the cap, tallest {}  generator_version {}",
                        report.config.dir,
                        report.fingerprint,
                        report.games.len(),
                        report.config.cap,
                        report.at_cap,
                        report.tallest_over_set,
                        report.manifest.generator_version
                    );
                }
            }
            Err(drift) => {
                if args.json {
                    let message = drift.to_string().replace('\\', "\\\\").replace('"', "\\\"");
                    println!(
                        "{{\"config\":\"{}\",\"ok\":false,\"error\":\"{}\"}}",
                        cfg.dir,
                        message.replace('\n', "\\n")
                    );
                } else {
                    eprintln!("{drift}");
                }
                code = 1;
            }
        }
    }
    // The pairing property needs both files, so it only runs when both are in
    // scope.
    if !single && code == 0 {
        if let Err(drift) = verify::run_all(&args.data) {
            eprintln!("{drift}");
            code = 1;
        }
    }
    if code == 0 {
        Ok(())
    } else {
        Err(code)
    }
}

fn fingerprint(args: &Args) -> Result<(), u8> {
    let mut code = 0u8;
    for cfg in &args.configs {
        let path = dataset::games_path(&args.data, cfg);
        match std::fs::read_to_string(&path) {
            Ok(text) => match dataset::parse(cfg, &text) {
                Ok(parsed) => println!(
                    "{} {} {}",
                    cfg.dir,
                    fingerprint::hex(fingerprint::fingerprint(&parsed.games)),
                    path.display()
                ),
                Err(error) => {
                    eprintln!("error: {} does not parse: {error}", path.display());
                    code = 1;
                }
            },
            Err(error) => {
                eprintln!("error: cannot read {}: {error}", path.display());
                code = 1;
            }
        }
    }
    if code == 0 {
        Ok(())
    } else {
        Err(code)
    }
}

fn show_game(args: &Args) -> Result<(), u8> {
    let Some(seed) = args.seed else {
        eprintln!("error: show needs --seed <n>");
        return Err(2);
    };
    let cfg = args.configs.first().copied().unwrap_or(dataset::GOAL8);
    let path = dataset::games_path(&args.data, &cfg);
    let text = std::fs::read_to_string(&path).map_err(|error| {
        eprintln!("error: cannot read {}: {error}", path.display());
        1u8
    })?;
    let parsed = dataset::parse(&cfg, &text).map_err(|error| {
        eprintln!("error: {} does not parse: {error}", path.display());
        1u8
    })?;
    let game = parsed
        .games
        .iter()
        .find(|game| game.seed == seed)
        .ok_or_else(|| {
            eprintln!("error: seed {seed} is not in {}", path.display());
            1u8
        })?;
    print!("{}", show::ascii(game));
    let _ = std::io::stdout().flush();
    Ok(())
}

// ------------------------------------------------------------------ gate

/// `bench gate <1..6|all> [--data <dir>] [--no-colour]`.
///
/// One command, one exit code, one block of named assertions per gate. Colour is
/// for a terminal only (`NO_COLOR` respected), exactly as `host-probe/verify.sh`
/// guards its own: a log in CI is read as text, not as escape codes.
fn gate_command(argv: &[String]) -> u8 {
    let mut selector = "all".to_string();
    let mut data: Option<PathBuf> = None;
    let mut colour = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--data" => {
                index += 1;
                match argv.get(index) {
                    Some(value) => data = Some(PathBuf::from(value)),
                    None => {
                        eprintln!(
                            "error: --data needs a directory

{USAGE}"
                        );
                        return 2;
                    }
                }
            }
            "--no-colour" | "--no-color" => colour = false,
            other if other.starts_with("--") => {
                eprintln!(
                    "error: unknown flag {other:?}

{USAGE}"
                );
                return 2;
            }
            other => selector = other.to_string(),
        }
        index += 1;
    }

    let mut options = GateOptions::resolve();
    if let Some(data) = data {
        options.paths.data = data;
    }
    let reports = match selector.as_str() {
        "all" => gate::run_all(&options),
        "1" | "2" | "3" | "4" | "5" | "6" => {
            vec![gate::run(
                selector.parse().expect("selected above"),
                &options,
            )]
        }
        other => {
            eprintln!(
                "error: unknown gate {other:?}; known: 1, 2, 3, 4, 5, 6, all

{USAGE}"
            );
            return 2;
        }
    };

    let in_ci = std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true");
    for (position, report) in reports.iter().enumerate() {
        if position > 0 {
            println!();
        }
        println!("{}", report.render_with(colour));
        if in_ci {
            for annotation in report.annotations() {
                println!("{annotation}");
            }
        }
    }
    let _ = std::io::stdout().flush();
    gate::worst_exit_code(&reports)
}

// ------------------------------------------------------------- solve --emit

/// The hidden half of gate 5: solve the fixed seeds and print one line per
/// `(configuration, seed)` to stdout, and nothing else.
///
/// ```text
/// goal8-p40-cap7 0 10713641301068153 0109,50105,30008,20100,40105
/// ```
///
/// `<config> <seed> <packed decimal> <Move words, or ->`, the words being
/// `(piece_index << 16) | (rotation << 8) | column` in lowercase hex. The
/// comparison unit is
/// the printed line, not the packed `i64`: the packed value carries the Move
/// *count* and not the Move order, so a determinism check on it alone could pass
/// while the returned moves differ.
///
/// The child reads nothing from `data/dataset`: it regenerates each game from its
/// Seed, so a Dataset that happens to be dirty cannot make two processes disagree.
fn solve_command(argv: &[String]) -> u8 {
    let mut emit = false;
    let mut configs: Option<Vec<Config>> = None;
    let mut seeds: Vec<u32> = vec![0];
    let mut beam = gate::GATE_BEAM;
    let mut index = 0;
    while index < argv.len() {
        let flag = argv[index].as_str();
        let mut take = |what: &str| -> Option<String> {
            index += 1;
            argv.get(index).cloned().or_else(|| {
                eprintln!("error: {flag} needs {what}");
                None
            })
        };
        match flag {
            "--emit" => emit = true,
            "--config" => {
                let Some(value) = take("a configuration name or all") else {
                    return 2;
                };
                if value == "all" {
                    configs = Some(dataset::CONFIGS.to_vec());
                } else {
                    let Some(cfg) = Config::from_name(&value) else {
                        eprintln!("error: unknown configuration {value:?}");
                        return 2;
                    };
                    configs.get_or_insert_with(Vec::new).push(cfg);
                }
            }
            "--seeds" => {
                let Some(value) = take("a seed list or range") else {
                    return 2;
                };
                match parse_seeds(&value) {
                    Ok(parsed) => seeds = parsed,
                    Err(message) => {
                        eprintln!("error: {message}");
                        return 2;
                    }
                }
            }
            "--beam" => {
                let Some(value) = take("a width") else {
                    return 2;
                };
                match value.parse() {
                    Ok(width) => beam = width,
                    Err(_) => {
                        eprintln!("error: --beam must be a number, found {value:?}");
                        return 2;
                    }
                }
            }
            other => {
                eprintln!(
                    "error: unknown argument {other:?}

{USAGE}"
                );
                return 2;
            }
        }
        index += 1;
    }
    if !emit {
        eprintln!(
            "error: solve needs --emit

{USAGE}"
        );
        return 2;
    }

    for cfg in configs.unwrap_or_else(|| dataset::CONFIGS.to_vec()) {
        for seed in &seeds {
            let game = match engine::generate_game(*seed, cfg.pieces, cfg.cap) {
                Ok(game) => game,
                Err(error) => {
                    eprintln!("error: seed {seed} is out of the Generator's domain ({error:?})");
                    return 1;
                }
            };
            let result = engine::solve(
                &game.board,
                game.sequence.as_slice(),
                &engine::SolveConfig::new(cfg.goal, beam),
                engine::shipped(),
            );
            let moves = if result.moves.is_empty() {
                "-".to_string()
            } else {
                result
                    .moves
                    .iter()
                    .map(|mv| format!("{:04x}", mv.packed()))
                    .collect::<Vec<_>>()
                    .join(",")
            };
            println!("{} {} {} {}", cfg.dir, seed, result.packed(), moves);
        }
    }
    let _ = std::io::stdout().flush();
    0
}

/// `0,42,512` or `0-99`, because a gate that names a seed list by hand should not
/// have to spell out a hundred numbers.
fn parse_seeds(text: &str) -> Result<Vec<u32>, String> {
    if let Some((first, last)) = text.split_once('-') {
        let first: u32 = first
            .trim()
            .parse()
            .map_err(|_| format!("--seeds range {text:?} has a first seed that is not a number"))?;
        let last: u32 = last
            .trim()
            .parse()
            .map_err(|_| format!("--seeds range {text:?} has a last seed that is not a number"))?;
        if last < first {
            return Err(format!("--seeds range {text:?} ends before it starts"));
        }
        return Ok((first..=last).collect());
    }
    text.split(',')
        .map(|seed| {
            seed.trim()
                .parse()
                .map_err(|_| format!("--seeds entry {seed:?} is not a number"))
        })
        .collect()
}

/// The probe a run recorded, if any. All-or-nothing, because a probe is evidence:
/// a width with no sample size is a number nobody can check, and a note with no
/// probe is prose about nothing.
fn overshoot(args: &Args) -> Result<Option<width::Overshoot>, String> {
    let Some((width, ms_p90)) = args.overshoot else {
        for (given, flag) in [
            (args.overshoot_samples.is_some(), "--overshoot-samples"),
            (args.overshoot_substrate.is_some(), "--overshoot-substrate"),
            (args.overshoot_note.is_some(), "--overshoot-note"),
        ] {
            if given {
                return Err(format!("{flag} needs --overshoot <width>=<p90_ms>"));
            }
        }
        return Ok(None);
    };
    let samples = args.overshoot_samples.ok_or_else(|| {
        "--overshoot needs --overshoot-samples <n>: a p90 without a sample size is not a measurement"
            .to_string()
    })?;
    Ok(Some(width::Overshoot {
        width,
        substrate: match args.overshoot_substrate.unwrap_or(MsSource::Native) {
            MsSource::Native => "native".to_string(),
            MsSource::Wasm => "wasm".to_string(),
        },
        samples,
        ms_p90,
        note: args.overshoot_note.clone().unwrap_or_default(),
    }))
}

/// `bench width-curve --summarise <artifact>`: re-derive the summary fields of a
/// curve that is already on disk.
///
/// The measured series is the expensive part and the part this command may not
/// touch, which is why it reparses the artifact, re-derives from what it found, and
/// writes only if every byte outside the summary block is unchanged. It is the
/// honest repair for a file whose derived fields were computed from the wrong list.
fn summarise_curve(path: &Path, out: &Path, probe: Option<width::Overshoot>) -> Result<(), u8> {
    match width::summarise_file(path, out, probe) {
        Ok(report) => {
            println!("summarised {}", path.display());
            for (label, summary) in [("before", &report.before), ("after ", &report.after)] {
                println!(
                    "  {label} width_cap {} (bound {}), complete {}, missing [{}]",
                    summary.width_cap,
                    match summary.cap_bound {
                        width::CapBound::Budget => "budget",
                        width::CapBound::Schedule => "schedule",
                    },
                    summary.complete,
                    summary
                        .missing_widths
                        .iter()
                        .map(|width| width.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            if !report.after.incomplete_reason.is_empty() {
                println!("  reason: {}", report.after.incomplete_reason);
            }
            println!(
                "wrote {} ({} measured points, quality fingerprint {} — all unchanged)",
                out.display(),
                report.artifact.points.len(),
                report.artifact.quality_fingerprint
            );
            Ok(())
        }
        Err(error) => {
            eprintln!("error: {error}");
            Err(2)
        }
    }
}

/// §9.5: measure the width curve and write the artifact the page reads.
///
/// The numbers are measured, not asserted: `bench width-curve` is the only thing
/// that writes `data/measurements/width-curve.json`. The command prints every
/// point as it goes, so a run that is cut short leaves a partial artifact on disk
/// that says which points it got.
///
/// `--summarise` is the one exception, and it is not a measurement: it re-derives
/// the fields the artifact computes from its own evidence (which widths are
/// missing, why the sweep stopped) and refuses to write if a measured number would
/// move.
fn width_curve(args: &Args) -> Result<(), u8> {
    let probe = match overshoot(args) {
        Ok(probe) => probe,
        Err(message) => {
            eprintln!("error: {message}");
            return Err(2);
        }
    };

    if let Some(path) = &args.summarise {
        // Writing back over the artifact it was given is the point: a copied file
        // is a different artifact, and the caller can still say `--out`.
        let out = args.out.clone().unwrap_or_else(|| path.clone());
        return summarise_curve(path, &out, probe);
    }

    let out = args.out.clone().unwrap_or_else(measurements_path);
    let options = WidthOptions {
        data: args.data.clone(),
        out: out.clone(),
        widths: args.widths.clone(),
        trials: args.trials,
        trial_seeds: args.seeds_per_trial,
        quality_games: args.quality_games,
        ms_source: args.ms,
        engine_commit: engine_commit(),
        machine: args.machine.clone().unwrap_or_else(machine_label),
        runtime: runtime_label(),
        reason: args
            .reason
            .clone()
            .unwrap_or_else(|| "initial measurement for ticket 06 (§9.5)".to_string()),
        quiet: false,
        now_secs: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0),
        overshoot: probe,
    };

    println!(
        "width-curve: {} doubling points, {} trials x {} seeds, {} quality games, {} ms",
        options.widths.len(),
        options.trials,
        options.trial_seeds,
        options.quality_games,
        match options.ms_source {
            MsSource::Native => "native",
            MsSource::Wasm => "wasm",
        }
    );
    match width::run(&options) {
        Ok(artifact) => {
            println!(
                "default_beam {} ({}, basis {}), width_cap {} (bound {}), quality axis {}",
                artifact.default_beam,
                match artifact.default_beam_source {
                    width::DefaultSource::Tied => "tie",
                    width::DefaultSource::SelfTie => "self",
                },
                artifact.default_beam_basis.as_str(),
                artifact.width_cap,
                match artifact.cap_bound {
                    width::CapBound::Budget => "budget",
                    width::CapBound::Schedule => "schedule",
                },
                artifact.quality_axis.as_str()
            );
            println!(
                "wrote {} (quality fingerprint {})",
                out.display(),
                artifact.quality_fingerprint
            );
            println!(
                "coverage: complete {}, missing [{}] — {}",
                artifact.complete,
                artifact
                    .missing_widths
                    .iter()
                    .map(|width| width.to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                if artifact.incomplete_reason.is_empty() {
                    "every pre-registered doubling point was measured"
                } else {
                    &artifact.incomplete_reason
                }
            );
            Ok(())
        }
        Err(error) => {
            eprintln!("error: {error}");
            Err(2)
        }
    }
}

/// The commit the curve is measured against: `git rev-parse --short HEAD`, or a
/// word that says the lookup failed rather than a lie.
fn engine_commit() -> String {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output();
    match output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => "unknown (git rev-parse failed)".to_string(),
    }
}

/// §9.3: a p90 without a box is not a measurement, so the label travels with it.
fn machine_label() -> String {
    // §9.3: "a p90 without a box is not a measurement". Ask the operating system
    // once, and say so plainly if it will not answer.
    let script = "$c=Get-CimInstance Win32_Processor|Select-Object -First 1;$s=Get-CimInstance Win32_ComputerSystem;$o=Get-CimInstance Win32_OperatingSystem;'{0}|{1}|{2}|{3}|{4}' -f $c.Name.Trim(),$c.NumberOfCores,$c.NumberOfLogicalProcessors,[math]::Round($s.TotalPhysicalMemory/1GB,1),$o.Caption.Trim()";
    if let Some(probe) = powershell(script) {
        let fields: Vec<&str> = probe.split('|').collect();
        if fields.len() == 5 {
            return format!(
                "{} ({}C/{}T), {} GB, {}",
                fields[0], fields[1], fields[2], fields[3], fields[4]
            );
        }
    }
    let logical = std::thread::available_parallelism()
        .map(|count| count.get().to_string())
        .unwrap_or_else(|_| "?".to_string());
    format!(
        "unknown CPU ({} logical cores), {} {}",
        logical,
        std::env::consts::OS,
        std::env::consts::ARCH
    )
}

fn powershell(script: &str) -> Option<String> {
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

/// What produced the numbers, on the box's own terms: the wasm path runs the
/// artifact through V8 (node), which is the engine headless Chrome runs, without
/// the DOM.
fn runtime_label() -> String {
    let node = std::process::Command::new("node").arg("--version").output();
    let node = match node {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => "node (version unknown)".to_string(),
    };
    format!(
        "{node} (V8: the engine headless Chrome runs, without the DOM); rustc 1.99.1 release bench",
    )
}

// ------------------------------------------------------------------- tune

/// `bench tune [flags]` and `bench tune --replay [<record>]` -- ticket 07.
///
/// Its own flag loop, like `gate` and `solve`, so the trainer cannot grow the
/// Dataset commands' argument surface and the two cannot drift.
fn tune_command(argv: &[String]) -> u8 {
    let mut options = tuning::Options::defaults();
    options.now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    let mut replay: Option<Option<PathBuf>> = None;
    let mut index = 0;
    while index < argv.len() {
        let flag = argv[index].to_string();
        let outcome: Result<(), String> = (|| {
            match flag.as_str() {
                "--seed" => {
                    let value =
                        flag_value(argv, &mut index, &flag, "a u64 seed, decimal or 0x-hex")?;
                    options.tune_seed = parse_u64(&value)
                        .ok_or_else(|| format!("--seed: {value:?} is not a u64"))?;
                }
                "--bed" => {
                    let value =
                        flag_value(argv, &mut index, &flag, "a bed name like goal12-p100-cap7")?;
                    options.bed = bed_from_name(&value)
                        .ok_or_else(|| format!("--bed: unknown bed {value:?}"))?;
                }
                "--candidates" => {
                    let value = flag_value(argv, &mut index, &flag, "a candidate count")?;
                    options.params.candidates = value
                        .parse()
                        .map_err(|_| format!("--candidates must be a number, found {value:?}"))?;
                }
                "--games" => {
                    let value = flag_value(argv, &mut index, &flag, "a games-per-candidate count")?;
                    options.params.games_per_candidate = value
                        .parse()
                        .map_err(|_| format!("--games must be a number, found {value:?}"))?;
                }
                "--iterations" => {
                    let value = flag_value(argv, &mut index, &flag, "an iteration count")?;
                    options.params.iterations = value
                        .parse()
                        .map_err(|_| format!("--iterations must be a number, found {value:?}"))?;
                }
                "--data" => {
                    options.data =
                        PathBuf::from(flag_value(argv, &mut index, &flag, "a directory")?)
                }
                "--out" => {
                    options.out = Some(PathBuf::from(flag_value(
                        argv, &mut index, &flag, "a file",
                    )?))
                }
                "--rows" => {
                    options.rows_out = Some(PathBuf::from(flag_value(
                        argv, &mut index, &flag, "a file",
                    )?))
                }
                "--quiet" => options.quiet = true,
                "--replay" => {
                    // `--replay` takes an optional path; the committed record is
                    // the default, which is the only case that matters.
                    let path = argv
                        .get(index + 1)
                        .filter(|next| !next.starts_with("--"))
                        .map(PathBuf::from);
                    if path.is_some() {
                        index += 1;
                    }
                    replay = Some(path);
                }
                other => return Err(format!("unknown argument {other:?}")),
            }
            Ok(())
        })();
        if let Err(message) = outcome {
            eprintln!(
                "error: {message}

{USAGE}"
            );
            return 2;
        }
        index += 1;
    }

    if let Some(path) = replay {
        let path = path.unwrap_or_else(tuning::record::record_path);
        println!("tune --replay {}", path.display());
        return match tuning::replay(&path, &options.data) {
            Ok((committed, replayed, same)) => {
                println!("committed table {committed}");
                println!("replayed  table {replayed}");
                if same {
                    println!("reproducible: the committed seed draws the committed table");
                    0
                } else {
                    eprintln!(
                        "REPLAY MISMATCH: the committed seed does not draw the committed table"
                    );
                    1
                }
            }
            Err(message) => {
                eprintln!("error: {message}");
                2
            }
        };
    }

    println!(
        "tune: seed 0x{:016x}, bed {}, N={} G={} K={}, tuning Split only",
        options.tune_seed,
        options.bed.name(),
        options.params.candidates,
        options.params.games_per_candidate,
        options.params.iterations
    );
    match tuning::run(&options) {
        Ok(outcome) => {
            println!(
                "run {} {}: iterations {} ({}), table {}",
                outcome.run,
                outcome.decision,
                outcome.iterations_run,
                outcome.stop_reason,
                outcome.table_fnv
            );
            for (name, stats) in &outcome.stats {
                println!(
                    "  {:<5} win {:.4} [{:.4}, {:.4}]  lines {:.4}  evals p50 {:.0} p90 {:.0}",
                    name,
                    stats.win_rate,
                    stats.win_rate_ci95[0],
                    stats.win_rate_ci95[1],
                    stats.mean_lines,
                    stats.evals_median,
                    stats.evals_p90
                );
            }
            println!(
                "  e2 vs e1: b={} c={} p={:.3e}, lines delta {:+.4} [{:+.4}, {:+.4}], win-rate axis {}",
                outcome.comparison.b,
                outcome.comparison.c,
                outcome.comparison.mcnemar_p,
                outcome.comparison.mean_lines_delta,
                outcome.comparison.mean_lines_ci95[0],
                outcome.comparison.mean_lines_ci95[1],
                outcome.win_rate_axis
            );
            println!(
                "  wrote {} and {} ({} s)",
                outcome.record_path.display(),
                outcome.rows_path.display(),
                outcome.wall_clock_seconds
            );
            if outcome.decision == "accepted" {
                0
            } else {
                eprintln!(
                    "REJECTED run {}: {} - {}",
                    outcome.run,
                    outcome.rejection_kind.clone().unwrap_or_default(),
                    outcome.rejection_reason.clone().unwrap_or_default()
                );
                1
            }
        }
        Err(message) => {
            eprintln!("error: {message}");
            2
        }
    }
}

/// One flag's value: `argv[index + 1]`, with the flag named in the error.
fn flag_value(
    argv: &[String],
    index: &mut usize,
    flag: &str,
    what: &str,
) -> Result<String, String> {
    *index += 1;
    argv.get(*index)
        .cloned()
        .ok_or_else(|| format!("{flag} needs a value ({what})"))
}

fn parse_u64(text: &str) -> Option<u64> {
    let trimmed = text.trim();
    match trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
    {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => trimmed.parse().ok(),
    }
}

fn bed_from_name(name: &str) -> Option<width::Bed> {
    [width::Bed::GOAL8, width::Bed::GOAL12, width::Bed::GOAL16]
        .into_iter()
        .find(|bed| bed.name() == name)
}
