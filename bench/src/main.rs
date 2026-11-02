//! `bench` -- the native Dataset tool (§6, §9.2 gate 1).
//!
//! ```text
//! bench export [--config <name|all>] [--data <dir>] [--accept-change "<reason>"] [--dry-run]
//! bench verify [--config <name|all>] [--data <dir>] [--json]
//! bench fingerprint [--config <name|all>] [--data <dir>]
//! bench show [--config <name>] --seed <n> [--data <dir>]
//! ```
//!
//! Exit codes: `0` clean, `1` the Dataset drifted (the gate's failure), `2` a
//! usage or I/O error. Argument parsing is here and nothing else is: every
//! subcommand is one call into the library, which is what the tests call too.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use bench::dataset::{self, Config};
use bench::{export, fingerprint, show, verify};

const USAGE: &str = "\
usage:
  bench export      [--config <name|all>] [--data <dir>] [--accept-change \"<reason>\"] [--dry-run]
  bench verify      [--config <name|all>] [--data <dir>] [--json]
  bench fingerprint [--config <name|all>] [--data <dir>]
  bench show        [--config <name>] --seed <n> [--data <dir>]

configurations: goal8-p40-cap7, goal12-p100-cap7
--data defaults to the repository's data/dataset.
exit codes: 0 clean, 1 Dataset drift, 2 usage or I/O error.";

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
