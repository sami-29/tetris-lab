//! `bench` -- the native Dataset tool (§6, §9.2 gate 1).
//!
//! ```text
//! bench export [--config <name|all>] [--data <dir>] [--accept-change "<reason>"] [--dry-run]
//! bench verify [--config <name|all>] [--data <dir>] [--json]
//! bench fingerprint [--config <name|all>] [--data <dir>]
//! bench show [--config <name>] --seed <n> [--data <dir>]
//! bench gate <1|2|3|4|5|6|all> [--data <dir>] [--no-colour]
//! bench solve --emit [--config <name|all>] [--seeds <a,b,c>] [--beam <n>] [--data <dir>]
//! ```
//!
//! Exit codes: `0` clean, `1` the Dataset drifted or a Gate failed, `2` a usage
//! or I/O error or a Gate whose prerequisite is missing. Argument parsing is here
//! and nothing else is: every subcommand is one call into the library, which is
//! what the tests call too.

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use bench::dataset::{self, Config};
use bench::gate::{self, Options};
use bench::{export, fingerprint, show, verify};

const USAGE: &str = "\
usage:
  bench export      [--config <name|all>] [--data <dir>] [--accept-change \"<reason>\"] [--dry-run]
  bench verify      [--config <name|all>] [--data <dir>] [--json]
  bench fingerprint [--config <name|all>] [--data <dir>]
  bench show        [--config <name>] --seed <n> [--data <dir>]
  bench gate        <1|2|3|4|5|6|all> [--data <dir>] [--no-colour]
  bench solve       --emit [--config <name|all>] [--seeds <a,b,c|a-b>] [--beam <n>]

configurations: goal8-p40-cap7, goal12-p100-cap7
--data defaults to the repository's data/dataset.
exit codes: 0 clean, 1 Dataset drift or a failed Gate, 2 usage, I/O, or a Gate
whose prerequisite is missing.";

struct Args {
    configs: Vec<Config>,
    data: PathBuf,
    accept_change: Option<String>,
    dry_run: bool,
    json: bool,
    seed: Option<u32>,
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
    let mut args = Args {
        configs: dataset::CONFIGS.to_vec(),
        data: dataset::root(),
        accept_change: None,
        dry_run: false,
        json: false,
        seed: None,
    };
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

    let mut options = Options::resolve();
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
/// goal8-p40-cap7 42 10713641301068303 0308,0f08,0407
/// ```
///
/// `<config> <seed> <packed decimal> <Move words, or ->`. The comparison unit is
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
