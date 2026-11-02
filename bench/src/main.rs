//! `bench` -- the native Dataset tool (§6, §9.2 gate 1).
//!
//! ```text
//! bench export [--config <name|all>] [--data <dir>] [--accept-change "<reason>"] [--dry-run]
//! bench verify [--config <name|all>] [--data <dir>] [--json]
//! bench fingerprint [--config <name|all>] [--data <dir>]
//! bench show [--config <name>] --seed <n> [--data <dir>]
//! bench width-curve [--widths <w,w,...>] [--trials <n>] [--seeds-per-trial <n>]
//!                   [--quality-games <n>] [--ms native|wasm] [--out <file>]
//!                   [--reason "<one line>"] [--machine "<label>"]
//! ```
//!
//! Exit codes: `0` clean, `1` the Dataset drifted (the gate's failure), `2` a
//! usage or I/O error. Argument parsing is here and nothing else is: every
//! subcommand is one call into the library, which is what the tests call too.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use bench::dataset::{self, Config};
use bench::width::{self, MsSource, Options, WIDTHS};
use bench::{export, fingerprint, show, verify};

const USAGE: &str = "\
usage:
  bench export      [--config <name|all>] [--data <dir>] [--accept-change \"<reason>\"] [--dry-run]
  bench verify      [--config <name|all>] [--data <dir>] [--json]
  bench fingerprint [--config <name|all>] [--data <dir>]
  bench show        [--config <name>] --seed <n> [--data <dir>]
  bench width-curve [--widths <w,w,...>] [--trials <n>] [--seeds-per-trial <n>]
                    [--quality-games <n>] [--ms native|wasm] [--out <file>]
                    [--reason \"<one line>\"] [--machine \"<label>\"]

configurations: goal8-p40-cap7, goal12-p100-cap7
width-curve defaults: every doubling point, 3 trials of 200 seeds, 1,000 quality
games, wasm milliseconds, written to data/measurements/width-curve.json.
--data defaults to the repository's data/dataset.
exit codes: 0 clean, 1 Dataset drift, 2 usage or I/O error.";

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
    out: PathBuf,
    reason: Option<String>,
    machine: Option<String>,
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
            out: measurements_path(),
            reason: None,
            machine: None,
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
            "--out" => args.out = PathBuf::from(take("a file")?),
            "--reason" => args.reason = Some(take("a one-line reason")?),
            "--machine" => args.machine = Some(take("a machine label")?),
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

/// §9.5: measure the width curve and write the artifact the page reads.
///
/// The numbers are measured, not asserted: `bench width-curve` is the only thing
/// that writes `data/measurements/width-curve.json`. The command prints every
/// point as it goes, so a run that is cut short leaves a partial artifact on disk
/// that says which points it got.
fn width_curve(args: &Args) -> Result<(), u8> {
    let options = Options {
        data: args.data.clone(),
        out: args.out.clone(),
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
                options.out.display(),
                artifact.quality_fingerprint
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
