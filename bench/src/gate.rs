//! The six Gates (§9.2, §9.3): one subcommand, one assertion vocabulary, one
//! exit-code convention.
//!
//! ```text
//! cargo run -p bench --release -- gate <1|2|3|4|5|6|all> [--data <dir>]
//! ```
//!
//! A Gate names itself when it fails, so a red run explains itself without a
//! stack trace: one indented line per named assertion, `✓` / `✗` / `·` (skipped)
//! / `!` (a non-blocking warn), a header naming the Gate, and one `PASS` / `FAIL` /
//! `CANNOT RUN` summary with counts and the wall clock -- `host-probe/verify.sh`'s
//! shape, which is the style the ticket asks for.
//!
//! Exit codes, as `verify.sh` uses them: **0** every assertion passed, **1** an
//! assertion failed, **2** a prerequisite is missing, so "the Gate says no" is
//! distinguishable from "the Gate never looked". `gate all` runs 1→6 and returns
//! the worst code.
//!
//! Nothing here is a second implementation of anything:
//!
//! * Gate 1 and the manifest half of gate 2 call [`crate::verify`], ticket 02's
//!   gate, so the Dataset has one verifier.
//! * Gates 3, 4 and 5 call [`engine::solve`] with the same [`engine::SolveConfig`]
//!   the page's worker does. Gate 3's *checking* is the new code, in
//!   [`crate::replay`].
//! * Gates 4 and 6 shell out to `web/scripts/gate-artifacts.mjs` for the two
//!   things Rust cannot do without a dependency (a wasm instance and gzip), and
//!   fail with a named `cannot run` when Node or the artifact is absent.
//!
//! Gate 6's second half is ticket 09's: [`crate::golden`] compares the
//! evaluations-to-solve and lines-to-solve vectors **exactly** against
//! `data/measurements/golden-evals.json`, after comparing the identity the golden is
//! keyed on (the Dataset fingerprints, the wasm hash, `DEFAULT_BEAM`, the
//! evaluator and the toggles). Its wall clock gets one loose, calibration-normalised
//! 4x bound and nothing else, so a slow box can only ever trip that line: §9.2's
//! "CI guards the fingerprint, not the clock". A missing or unreadable golden is a
//! `✗`, not a `·` — ticket 03's placeholder is gone, and a gate that cannot compare
//! must say so.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::dataset::{self, Config, Manifest};
use crate::{replay, verify};

/// The width gates 3, 4 and 5 run at. Ticket 01 verified this width and it is
/// measurably cheap; `DEFAULT_BEAM` is an output of the width curve (§9.5) and
/// does not exist yet. Parity, replay and determinism are properties of the code
/// path, not of the width.
pub const GATE_BEAM: u32 = 25;

/// §9.2 gate 4's "fixed 100-seed subset of the test split". The test split is
/// 0–999, so this is its first hundred seeds, and it is fixed for the same reason
/// a golden value is: a gate that samples differently every run cannot be a gate.
pub const PARITY_SEEDS: (u32, u32) = (0, 99);

/// §9.2 gate 5's seeds: both ends of the Dataset, the seed every example in the
/// notes uses (42), the documented bit-flip victim (512), and the first seed of
/// the tuning split.
pub const DETERMINISM_SEEDS: [u32; 5] = [0, 42, 512, 1000, 1999];

/// §9.3: wasm raw size warns above 100 KB and fails above 150 KB. KiB, to match
/// `web/scripts/build-wasm.mjs`'s existing `100 * 1024`, so there are not two
/// definitions of the same budget.
pub const WASM_WARN_BYTES: u64 = 100 * 1024;
/// §9.3's wasm fail line.
pub const WASM_FAIL_BYTES: u64 = 150 * 1024;
/// §9.3: whole page transferred gzip fails above 150 KB. No warn tier exists for
/// this budget, so none is invented.
pub const PAGE_FAIL_BYTES: u64 = 150 * 1024;

/// Ticket 09's file. Only its *existence* is checked, on purpose: an empty `{}`
/// must trip the placeholder too.
pub const GOLDEN_FILE: &str = "golden-evals.json";

/// The command that builds the missing artifact, quoted by every `cannot run`
/// that needs it.
pub const BUILD_WASM_HINT: &str = "npm --prefix web run build:wasm";

/// The command that builds the whole page, quoted when `web/dist` is missing.
pub const BUILD_PAGE_HINT: &str = "npm --prefix web run build";

/// The command that re-exports the Dataset, quoted by gate 1's hint.
pub const RE_EXPORT_HINT: &str =
    "cargo run -p bench --release -- export --accept-change \"<one line reason>\"";

/// The Gates, in the order §9.2 lists them.
pub const GATES: [u8; 6] = [1, 2, 3, 4, 5, 6];

// --------------------------------------------------------------------- paths

/// Every path the Gates read. Resolved from this crate's own directory, so the
/// Gates behave the same from the workspace root, from `web/`, and from CI.
#[derive(Clone, Debug)]
pub struct Paths {
    /// `data/dataset`.
    pub data: PathBuf,
    /// `web/pkg/engine.wasm`: the artifact the page loads, not a cargo output.
    pub wasm: PathBuf,
    /// `web/dist`: the built page, whose assets gate 6 sums.
    pub dist: PathBuf,
    /// `web/scripts/gate-artifacts.mjs`: the wasm and gzip half of gates 4 and 6.
    pub node_script: PathBuf,
    /// `data/measurements`: where ticket 09's golden lands.
    pub measurements: PathBuf,
}

impl Paths {
    /// The repository's own paths.
    pub fn resolve() -> Paths {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("bench/ always has a parent: the workspace root")
            .to_path_buf();
        Paths {
            data: root.join("data").join("dataset"),
            wasm: root.join("web").join("pkg").join("engine.wasm"),
            dist: root.join("web").join("dist"),
            node_script: root.join("web").join("scripts").join("gate-artifacts.mjs"),
            measurements: root.join("data").join("measurements"),
        }
    }

    /// Ticket 09's golden evaluations file.
    pub fn golden(&self) -> PathBuf {
        self.measurements.join(GOLDEN_FILE)
    }
}

/// What a Gate run needs: where the files are, and which binary gate 5 spawns
/// twice (itself, so the child is the same build the parent measured).
#[derive(Clone, Debug)]
pub struct Options {
    pub paths: Paths,
    pub exe: PathBuf,
}

impl Options {
    pub fn resolve() -> Options {
        Options {
            paths: Paths::resolve(),
            exe: std::env::current_exe().unwrap_or_default(),
        }
    }
}

// ------------------------------------------------------------------- report

/// One assertion's outcome. `Skip` is not an outcome for an assertion: it is a
/// half of the gate that has no assertion yet (gate 6b), and it is not counted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Pass,
    Fail,
    /// A prerequisite is missing: the gate never looked.
    CannotRun,
    /// Deliberately not asserted yet, and reported so it cannot be forgotten.
    Skip,
}

/// One named assertion.
#[derive(Clone, Debug)]
pub struct Check {
    pub name: String,
    pub state: State,
    /// The measured value on a pass, the diagnosis on a failure.
    pub subject: String,
    /// Extra indented lines: a drift message, both values, a hint's context.
    pub lines: Vec<String>,
    pub hint: Option<String>,
    /// A pass that should be looked at (gate 6's 100 KiB warn line).
    pub warn: bool,
}

impl Check {
    fn new(name: impl Into<String>, state: State, subject: impl Into<String>) -> Check {
        Check {
            name: name.into(),
            state,
            subject: subject.into(),
            lines: Vec::new(),
            hint: None,
            warn: false,
        }
    }

    pub fn pass(name: impl Into<String>, subject: impl Into<String>) -> Check {
        Check::new(name, State::Pass, subject)
    }

    pub fn warn(name: impl Into<String>, subject: impl Into<String>) -> Check {
        Check {
            warn: true,
            ..Check::new(name, State::Pass, subject)
        }
    }

    pub fn fail(name: impl Into<String>) -> Check {
        Check::new(name, State::Fail, "")
    }

    pub fn cannot_run(name: impl Into<String>, subject: impl Into<String>) -> Check {
        Check::new(name, State::CannotRun, subject)
    }

    pub fn skip(name: impl Into<String>, subject: impl Into<String>) -> Check {
        Check::new(name, State::Skip, subject)
    }

    pub fn subject(mut self, subject: impl Into<String>) -> Check {
        self.subject = subject.into();
        self
    }

    pub fn lines<I, S>(mut self, lines: I) -> Check
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.lines = lines.into_iter().map(Into::into).collect();
        self
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Check {
        self.hint = Some(hint.into());
        self
    }

    /// Whether this assertion counts towards the summary.
    fn counted(&self) -> bool {
        self.state != State::Skip
    }
}

/// One Gate's run: its header, its assertions, and how long it took.
#[derive(Clone, Debug)]
pub struct Report {
    pub gate: u8,
    pub title: String,
    /// What the Gate was run against, printed in the header.
    pub subject: String,
    pub checks: Vec<Check>,
    pub elapsed: Duration,
}

impl Report {
    pub fn new(gate: u8, title: impl Into<String>, subject: impl Into<String>) -> Report {
        Report {
            gate,
            title: title.into(),
            subject: subject.into(),
            checks: Vec::new(),
            elapsed: Duration::ZERO,
        }
    }

    pub fn push(&mut self, check: Check) {
        self.checks.push(check);
    }

    /// Assertions that count: everything but a `·` line.
    pub fn counted(&self) -> usize {
        self.checks.iter().filter(|check| check.counted()).count()
    }

    pub fn failed(&self) -> usize {
        self.checks
            .iter()
            .filter(|check| check.state == State::Fail)
            .count()
    }

    pub fn cannot_run(&self) -> usize {
        self.checks
            .iter()
            .filter(|check| check.state == State::CannotRun)
            .count()
    }

    pub fn skipped(&self) -> usize {
        self.checks
            .iter()
            .filter(|check| check.state == State::Skip)
            .count()
    }

    /// Green: nothing failed and nothing refused to run.
    pub fn passed(&self) -> bool {
        self.failed() == 0 && self.cannot_run() == 0
    }

    /// 0 green, 1 the Gate says no, 2 the Gate never looked.
    pub fn exit_code(&self) -> u8 {
        if self.cannot_run() > 0 {
            2
        } else if self.failed() > 0 {
            1
        } else {
            0
        }
    }

    /// The assertion whose name contains `needle`, for a caller that wants one.
    pub fn check(&self, needle: &str) -> Option<&Check> {
        self.checks.iter().find(|check| check.name.contains(needle))
    }

    /// Plain text: no colour, which is what a log and a test both want.
    pub fn render(&self) -> String {
        self.render_with(false)
    }

    /// The Gate's block: header, one line per assertion, summary.
    pub fn render_with(&self, colour: bool) -> String {
        let mut out = String::with_capacity(256 + self.checks.len() * 96);
        out.push_str(&format!(
            "gate {} · {} · {}",
            self.gate, self.title, self.subject
        ));
        for check in &self.checks {
            out.push('\n');
            match check.state {
                State::Pass => {
                    let mark = if check.warn { "!" } else { "✓" };
                    let code = if check.warn { "33" } else { "32" };
                    out.push_str(&format!("  {} {}", tint(colour, code, mark), check.name));
                    if !check.subject.is_empty() {
                        out.push_str(&format!(" · {}", check.subject));
                    }
                }
                State::Skip => {
                    out.push_str(&format!("  {} {}", tint(colour, "2", "·"), check.name));
                    if !check.subject.is_empty() {
                        out.push_str(&format!(" · {}", check.subject));
                    }
                }
                State::Fail | State::CannotRun => {
                    // The Gate's number is on every red line: a pasted ✗ says
                    // which of the six broke without its header.
                    out.push_str(&format!(
                        "  {} gate {} · {}",
                        tint(colour, "31", "✗"),
                        self.gate,
                        check.name
                    ));
                    if check.state == State::CannotRun {
                        out.push_str(" · cannot run");
                    }
                    if !check.subject.is_empty() {
                        out.push_str(&format!(" · {}", check.subject));
                    }
                    for line in &check.lines {
                        out.push_str(&format!("\n      {line}"));
                    }
                    if let Some(hint) = &check.hint {
                        out.push_str(&format!("\n      hint: {hint}"));
                    }
                }
            }
        }
        out.push('\n');
        let summary = if self.cannot_run() > 0 {
            format!(
                "{} gate {} · {} of {} checks could not run",
                tint(colour, "31", "CANNOT RUN"),
                self.gate,
                self.cannot_run(),
                self.counted()
            )
        } else if self.failed() > 0 {
            format!(
                "{} gate {} · {} of {} checks",
                tint(colour, "31", "FAIL"),
                self.gate,
                self.failed(),
                self.counted()
            )
        } else {
            format!(
                "{} gate {} · {} checks",
                tint(colour, "32", "PASS"),
                self.gate,
                self.counted()
            )
        };
        out.push_str(&format!(
            "{summary} · {} skipped · {}",
            self.skipped(),
            seconds(self.elapsed)
        ));
        out
    }

    /// GitHub Actions annotations. They do not change the exit code; they put the
    /// failing assertion in the run summary and on the pull request's file view.
    pub fn annotations(&self) -> Vec<String> {
        let mut out = Vec::new();
        for check in &self.checks {
            match check.state {
                State::Fail | State::CannotRun => out.push(format!(
                    "::error title=gate {} · {}::{}",
                    self.gate,
                    escape_annotation(&check.name),
                    escape_annotation(&check.subject)
                )),
                State::Pass if check.warn => out.push(format!(
                    "::warning title=gate {} · {}::{}",
                    self.gate,
                    escape_annotation(&check.name),
                    escape_annotation(&check.subject)
                )),
                _ => {}
            }
        }
        out
    }
}

fn escape_annotation(text: &str) -> String {
    text.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

fn tint(colour: bool, code: &str, text: &str) -> String {
    if colour {
        format!("\u{1b}[{code}m{text}\u{1b}[0m")
    } else {
        text.to_string()
    }
}

fn seconds(elapsed: Duration) -> String {
    format!("{:.2} s", elapsed.as_secs_f64())
}

// ----------------------------------------------------------------- the gates

/// Run one Gate. `GATES` lists the valid numbers; anything else is a usage error
/// reported as a failed assertion rather than a panic.
pub fn run(gate: u8, options: &Options) -> Report {
    let started = Instant::now();
    let mut report = match gate {
        1 => generator_fingerprint(&options.paths),
        2 => manifest_agreement(&options.paths),
        3 => replay_validity(&options.paths),
        4 => native_wasm_parity(&options.paths),
        5 => determinism(options),
        6 => budgets(options),
        other => {
            let mut report = Report::new(other, "unknown gate", String::new());
            report.push(
                Check::fail(format!("gate {other}"))
                    .subject("no such gate")
                    .hint("known gates: 1, 2, 3, 4, 5, 6, all"),
            );
            report
        }
    };
    report.elapsed = started.elapsed();
    report
}

/// Every Gate, in §9.2's order.
pub fn run_all(options: &Options) -> Vec<Report> {
    GATES.iter().map(|gate| run(*gate, options)).collect()
}

/// For `gate all`: 2 beats 1 beats 0, because the Gate that never looked is the
/// one to fix first.
pub fn worst_exit_code(reports: &[Report]) -> u8 {
    reports.iter().map(Report::exit_code).max().unwrap_or(0)
}

// ------------------------------------------------- gate 1: the fingerprint

/// §9.2 gate 1: every committed configuration regenerated in memory, compared
/// both bit for bit and by fingerprint, naming the first game that moved.
fn generator_fingerprint(paths: &Paths) -> Report {
    let mut report = Report::new(1, "generator fingerprint", paths.data.display().to_string());
    if !paths.data.is_dir() {
        report.push(
            Check::cannot_run("data/dataset", format!("{} is missing", paths.data.display()))
                .hint("the Dataset is committed; a missing directory means the files were never checked out, not that the Generator moved"),
        );
        return report;
    }
    for cfg in dataset::CONFIGS {
        match verify::check_config(&cfg, &paths.data) {
            Ok(done) => {
                report.push(Check::pass(
                    format!("{} · regenerates bit for bit", cfg.dir),
                    format!("{} games, digest {}", done.games.len(), done.fingerprint),
                ));
                report.push(Check::pass(
                    format!("{} · digest agrees", cfg.dir),
                    format!("file == regenerated == manifest, {}", done.fingerprint),
                ));
            }
            Err(drift) => {
                // The drift message already names the first game, the field, both
                // values and both digests: print it whole rather than paraphrase.
                report.push(
                    Check::fail(format!("{} · regenerates bit for bit", cfg.dir)).lines(
                        drift
                            .to_string()
                            .lines()
                            .map(|line| line.to_string())
                            .collect::<Vec<_>>(),
                    ),
                );
                report.push(
                    Check::fail(format!("{} · digest agrees", cfg.dir))
                        .subject("not compared: the games above already disagree")
                        .hint(RE_EXPORT_HINT),
                );
            }
        }
    }
    report
}

// ---------------------------------------------- gate 2: manifest agreement

/// §9.2 gate 2: every Dataset file's manifest agrees with the directory name, the
/// file's own header, and both Splits.
fn manifest_agreement(paths: &Paths) -> Report {
    let mut report = Report::new(
        2,
        "manifest agreement",
        format!(
            "{} · test {}-{}, tuning {}-{}",
            paths.data.display(),
            dataset::TEST_SPLIT.0,
            dataset::TEST_SPLIT.1,
            dataset::TUNING_SPLIT.0,
            dataset::TUNING_SPLIT.1
        ),
    );
    if !paths.data.is_dir() {
        report.push(Check::cannot_run(
            "data/dataset",
            format!("{} is missing", paths.data.display()),
        ));
        return report;
    }
    for cfg in dataset::CONFIGS {
        config_manifest_checks(&mut report, &cfg, paths);
    }
    report
}

fn config_manifest_checks(report: &mut Report, cfg: &Config, paths: &Paths) {
    let games_path = dataset::games_path(&paths.data, cfg);
    let manifest_path = dataset::manifest_path(&paths.data, cfg);
    if !games_path.exists() {
        report.push(
            Check::cannot_run(
                format!("{} · data/dataset", cfg.dir),
                format!("{} is missing", games_path.display()),
            )
            .hint("the Dataset is committed; checkout the files or re-export"),
        );
        return;
    }
    let text = match fs::read_to_string(&games_path) {
        Ok(text) => text,
        Err(error) => {
            report.push(
                Check::fail(format!("{} · seed coverage", cfg.dir))
                    .subject(format!("{} cannot be read: {error}", games_path.display())),
            );
            return;
        }
    };
    // `dataset::parse` is what establishes coverage as well as format: an
    // unparseable file cannot have its seeds checked, and its own message names
    // the line and the seed that is missing.
    let parsed = match dataset::parse(cfg, &text) {
        Ok(parsed) => parsed,
        Err(error) => {
            report.push(
                Check::fail(format!("{} · seed coverage", cfg.dir))
                    .subject("the file does not parse, so its seeds cannot be checked")
                    .lines([error.to_string()]),
            );
            return;
        }
    };

    let canonical = dataset::header_line(cfg);
    if parsed.header_line == canonical {
        report.push(Check::pass(
            format!("{} · header line is canonical", cfg.dir),
            "byte for byte",
        ));
    } else {
        report.push(
            Check::fail(format!("{} · header line is canonical", cfg.dir))
                .subject("the header is not the canonical line")
                .lines([
                    format!("expected: {canonical}"),
                    format!("actual:   {}", parsed.header_line),
                ]),
        );
    }

    let manifest_text = match fs::read_to_string(&manifest_path) {
        Ok(text) => text,
        Err(error) => {
            report.push(
                Check::fail(format!("{} · manifest parses", cfg.dir)).subject(format!(
                    "{} cannot be read: {error}",
                    manifest_path.display()
                )),
            );
            return;
        }
    };
    let manifest = match Manifest::parse(&manifest_text) {
        Ok(manifest) => manifest,
        Err(message) => {
            report.push(
                Check::fail(format!("{} · manifest parses", cfg.dir))
                    .subject("the manifest is not valid JSON of the §6 shape")
                    .lines([message]),
            );
            return;
        }
    };

    // The manifest against the directory name and against the file's own header.
    for (field, manifest_value, wanted) in [
        ("format", manifest.format, dataset::FORMAT),
        ("goal", manifest.goal, cfg.goal),
        ("pieces", manifest.pieces, cfg.pieces),
        ("cap", manifest.cap, cfg.cap),
        (
            "game_count",
            manifest.game_count,
            dataset::GAME_COUNT as u32,
        ),
        ("seeds.first", manifest.seeds.0, dataset::SEED_FIRST),
        ("seeds.last", manifest.seeds.1, dataset::SEED_LAST),
        ("header goal", parsed.header.goal, cfg.goal),
        ("header pieces", parsed.header.pieces, cfg.pieces),
        ("header cap", parsed.header.cap, cfg.cap),
    ] {
        field_check(report, cfg, field, wanted, manifest_value);
    }

    // Both Splits, each in the manifest and in the file.
    for (label, declared, expected) in [
        ("test", manifest.test_split, dataset::TEST_SPLIT),
        ("tuning", manifest.tuning_split, dataset::TUNING_SPLIT),
    ] {
        let name = format!("{} · split {label}", cfg.dir);
        let mut lines = Vec::new();
        if declared != expected {
            lines.push(format!(
                "declared {}-{}, expected {}-{}",
                declared.0, declared.1, expected.0, expected.1
            ));
        }
        let wanted = (expected.1 - expected.0 + 1) as usize;
        let mut present = 0usize;
        let mut first_seen: Option<u32> = None;
        let mut last_seen: Option<u32> = None;
        for game in &parsed.games {
            if game.seed >= expected.0 && game.seed <= expected.1 {
                present += 1;
                first_seen.get_or_insert(game.seed);
                last_seen = Some(game.seed);
            }
        }
        if present != wanted {
            lines.push(format!(
                "the file holds {present} games in that range, expected {wanted}"
            ));
        }
        if first_seen != Some(expected.0) || last_seen != Some(expected.1) {
            lines.push(format!(
                "the file's first and last seed in that range are {first_seen:?} and {last_seen:?}"
            ));
        }
        if lines.is_empty() {
            report.push(Check::pass(
                name,
                format!("{}-{} and the file holds it", expected.0, expected.1),
            ));
        } else {
            report.push(Check::fail(name).lines(lines));
        }
    }

    // The two Splits partition the seeds: disjoint, contiguous, covering.
    let test = manifest.test_split;
    let tuning = manifest.tuning_split;
    let partition = test.1 + 1 == tuning.0
        && test.0 == manifest.seeds.0
        && tuning.1 == manifest.seeds.1
        && test.0 <= test.1
        && tuning.0 <= tuning.1;
    if partition {
        report.push(Check::pass(
            format!("{} · splits partition the seeds", cfg.dir),
            format!(
                "{}-{} / {}-{} covers {}-{} exactly once",
                test.0, test.1, tuning.0, tuning.1, manifest.seeds.0, manifest.seeds.1
            ),
        ));
    } else {
        report.push(
            Check::fail(format!("{} · splits partition the seeds", cfg.dir)).lines([format!(
                "test {}-{}, tuning {}-{}, seeds {}-{}: the splits must be disjoint and cover every seed",
                test.0, test.1, tuning.0, tuning.1, manifest.seeds.0, manifest.seeds.1
            )]),
        );
    }

    // Every seed exactly once, ascending and contiguous, and the declaration
    // matches what the file holds.
    let mut coverage = Vec::new();
    if parsed.games.len() != dataset::GAME_COUNT {
        coverage.push(format!(
            "the file holds {} games, expected {}",
            parsed.games.len(),
            dataset::GAME_COUNT
        ));
    }
    for (offset, game) in parsed.games.iter().enumerate() {
        let expected = dataset::SEED_FIRST + offset as u32;
        if game.seed != expected {
            coverage.push(format!(
                "seed {expected} is missing: the file jumps to {}",
                game.seed
            ));
            break;
        }
    }
    if parsed.games.first().map(|game| game.seed) != Some(manifest.seeds.0) {
        coverage.push(format!(
            "the manifest's first seed is {}, the file's is {:?}",
            manifest.seeds.0,
            parsed.games.first().map(|game| game.seed)
        ));
    }
    if parsed.games.last().map(|game| game.seed) != Some(manifest.seeds.1) {
        coverage.push(format!(
            "the manifest's last seed is {}, the file's is {:?}",
            manifest.seeds.1,
            parsed.games.last().map(|game| game.seed)
        ));
    }
    if coverage.is_empty() {
        report.push(Check::pass(
            format!("{} · seed coverage", cfg.dir),
            format!(
                "{} seeds, each exactly once, {}-{} contiguous",
                parsed.games.len(),
                manifest.seeds.0,
                manifest.seeds.1
            ),
        ));
    } else {
        report.push(Check::fail(format!("{} · seed coverage", cfg.dir)).lines(coverage));
    }

    let reference = dataset::reference_path(&paths.data, cfg);
    if reference.exists() {
        report.push(Check::pass(
            format!("{} · reference.jsonl exists", cfg.dir),
            format!("{} bytes, append-only", size_of(&reference)),
        ));
    } else {
        report.push(
            Check::fail(format!("{} · reference.jsonl exists", cfg.dir))
                .subject(format!("{} is missing", reference.display()))
                .hint("cargo run -p bench --release -- export"),
        );
    }

    // §6.3: a Dataset file that a `.gitignore` pattern would swallow passes on a
    // machine that already has it and fails on a fresh clone. Cheap to check, and
    // the failure mode is invisible otherwise.
    let root = paths
        .data
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let watched: Vec<PathBuf> = dataset::Kind::ALL
        .iter()
        .map(|kind| dataset::path(&paths.data, cfg, *kind))
        .collect();
    match git_ignored(&root, &watched) {
        Some(ignored) if ignored.is_empty() => report.push(Check::pass(
            format!("{} · nothing is ignored", cfg.dir),
            "git check-ignore finds no pattern for any Dataset file",
        )),
        Some(ignored) => report.push(
            Check::fail(format!("{} · nothing is ignored", cfg.dir)).lines(
                ignored
                    .iter()
                    .map(|path| format!("ignored: {}", path.display()))
                    .collect::<Vec<_>>(),
            ),
        ),
        None => report.push(Check::skip(
            format!("{} · nothing is ignored", cfg.dir),
            "git is not available here, so the ignore guard was not run",
        )),
    }
}

fn field_check(
    report: &mut Report,
    cfg: &Config,
    field: &str,
    expected: impl ToString,
    actual: impl ToString,
) {
    let expected = expected.to_string();
    let actual = actual.to_string();
    let name = format!("{} · manifest field {field}", cfg.dir);
    if expected == actual {
        report.push(Check::pass(name, expected));
    } else {
        report.push(Check::fail(name).lines([
            format!("expected: {expected}"),
            format!("actual:   {actual}"),
        ]));
    }
}

/// Which of `paths` git would ignore. `None` means git could not be asked (not
/// installed, or the paths are outside the repository), which is a `·` line and
/// not a failure: the gates must run in a directory without a checkout.
fn git_ignored(root: &Path, paths: &[PathBuf]) -> Option<Vec<PathBuf>> {
    if root.as_os_str().is_empty() || !root.join(".git").exists() {
        return None;
    }
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(root)
        .arg("check-ignore")
        .arg("--no-index")
        .arg("--")
        .args(paths)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let output = command.output().ok()?;
    match output.status.code() {
        // 0: at least one path is ignored. 1: none is. Anything else: not a repo.
        Some(0) => Some(
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(PathBuf::from)
                .collect(),
        ),
        Some(1) => Some(Vec::new()),
        _ => None,
    }
}

fn size_of(path: &Path) -> u64 {
    fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

// ------------------------------------------------ gate 3: replay validity

/// §9.2 gate 3: every Move list the Search returns replays on the independent
/// simulator to the claimed Lines, with no overlap and a legal board.
fn replay_validity(paths: &Paths) -> Report {
    let mut report = Report::new(
        3,
        "replay validity",
        format!(
            "seeds {}-{} · beam {} · {}",
            PARITY_SEEDS.0,
            PARITY_SEEDS.1,
            GATE_BEAM,
            paths.data.display()
        ),
    );
    if !paths.data.is_dir() {
        report.push(Check::cannot_run(
            "data/dataset",
            format!("{} is missing", paths.data.display()),
        ));
        return report;
    }
    for cfg in dataset::CONFIGS {
        let mut won = 0usize;
        let mut lost = 0usize;
        let mut lines_total: u64 = 0;
        let mut replay_failures: Vec<String> = Vec::new();
        let mut line_failures: Vec<String> = Vec::new();
        let mut loss_failures: Vec<String> = Vec::new();

        for seed in PARITY_SEEDS.0..=PARITY_SEEDS.1 {
            let game = match engine::generate_game(seed, cfg.pieces, cfg.cap) {
                Ok(game) => game,
                Err(error) => {
                    line_failures
                        .push(format!("seed {seed}: the Generator refused it ({error:?})"));
                    continue;
                }
            };
            let result = engine::solve(
                &game.board,
                game.sequence.as_slice(),
                &engine::SolveConfig::new(cfg.goal, GATE_BEAM),
                engine::shipped(),
            );
            if !result.won {
                lost += 1;
                if !result.moves.is_empty() {
                    loss_failures.push(format!(
                        "seed {seed}: a lost Solve returned {} moves; a loss returns none",
                        result.moves.len()
                    ));
                }
                continue;
            }
            won += 1;
            if result.moves.len() > cfg.pieces as usize {
                replay_failures.push(format!(
                    "seed {seed}: {} moves for a {}-piece game",
                    result.moves.len(),
                    cfg.pieces
                ));
            }
            match replay::replay(&game.board, game.sequence.as_slice(), &result.moves) {
                Ok(done) => {
                    lines_total += done.lines as u64;
                    if done.lines != result.lines {
                        line_failures.push(format!(
                            "seed {seed}: replayed {} lines, the Solve claims {}",
                            done.lines, result.lines
                        ));
                    }
                    if done.lines < cfg.goal {
                        line_failures.push(format!(
                            "seed {seed}: {} lines is short of the goal {}",
                            done.lines, cfg.goal
                        ));
                    }
                    if done.full_rows != 0 {
                        replay_failures.push(format!(
                            "seed {seed}: {} full rows left on the board",
                            done.full_rows
                        ));
                    }
                }
                Err(error) => replay_failures.push(format!("seed {seed}: {error}")),
            }
        }

        let seeds = PARITY_SEEDS.1 - PARITY_SEEDS.0 + 1;
        push_outcome(
            &mut report,
            format!("{} · replay", cfg.dir),
            replay_failures,
            format!("{won} won of {seeds}, every placement legal, no row left full"),
        );
        push_outcome(
            &mut report,
            format!("{} · lines", cfg.dir),
            line_failures,
            format!(
                "{lines_total} lines replayed == claimed, every win >= goal {}",
                cfg.goal
            ),
        );
        push_outcome(
            &mut report,
            format!("{} · losses are empty", cfg.dir),
            loss_failures,
            format!("{lost} lost of {seeds}, every Move list empty"),
        );
    }
    report
}

/// One aggregate assertion: pass with the measured summary, or fail naming the
/// first offender and counting the rest.
fn push_outcome(report: &mut Report, name: String, failures: Vec<String>, summary: String) {
    if failures.is_empty() {
        report.push(Check::pass(name, summary));
        return;
    }
    let mut check = Check::fail(name).subject(failures[0].clone());
    let mut lines: Vec<String> = Vec::new();
    if failures.len() > 1 {
        lines.push(format!("and {} more like it", failures.len() - 1));
    }
    lines.extend(failures.iter().skip(1).take(4).cloned());
    check = check.lines(lines);
    report.push(check);
}

// ------------------------------------------------------ gate 4: parity

/// One row of gate 4's comparison, from either side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Outcome {
    status: u8,
    won: bool,
    evaluations: u32,
    lines: u32,
    moves: u32,
}

impl Outcome {
    fn from_native(result: &engine::SolveResult) -> Outcome {
        Outcome {
            status: result.status.code(),
            won: result.won,
            evaluations: result.evaluations,
            lines: result.lines,
            moves: result.moves.len() as u32,
        }
    }
}

/// §9.2 gate 4: the real artifact through the real loader, against the native
/// solve, on a fixed 100-seed subset of the test Split.
fn native_wasm_parity(paths: &Paths) -> Report {
    let mut report = Report::new(
        4,
        "native/wasm parity",
        format!(
            "seeds {}-{} · beam {}",
            PARITY_SEEDS.0, PARITY_SEEDS.1, GATE_BEAM
        ),
    );
    if !paths.wasm.exists() {
        report.push(
            Check::cannot_run(
                "wasm artifact",
                format!("{} is missing", paths.wasm.display()),
            )
            .hint(BUILD_WASM_HINT),
        );
        return report;
    }
    if !paths.node_script.exists() {
        report.push(
            Check::cannot_run(
                "node harness",
                format!("{} is missing", paths.node_script.display()),
            )
            .hint("the wasm half of gate 4 runs in Node"),
        );
        return report;
    }

    let mut command = Command::new(node_binary());
    command
        .arg(&paths.node_script)
        .arg("parity")
        .arg("--seeds")
        .arg(format!("{}-{}", PARITY_SEEDS.0, PARITY_SEEDS.1))
        .arg("--beam")
        .arg(GATE_BEAM.to_string());
    for cfg in dataset::CONFIGS {
        command.arg("--config").arg(format!(
            "{}:{}:{}:{}",
            cfg.dir, cfg.goal, cfg.pieces, cfg.cap
        ));
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = match command.output() {
        Ok(output) => output,
        Err(error) => {
            report.push(
                Check::cannot_run(
                    "node",
                    format!("cannot spawn {}: {error}", node_binary().to_string_lossy()),
                )
                .hint("Node 24 is the runner for the wasm half of gates 4 and 6"),
            );
            return report;
        }
    };
    if !output.status.success() {
        report.push(
            Check::cannot_run(
                "node harness",
                format!(
                    "gate-artifacts.mjs parity exited with {:?}",
                    output.status.code()
                ),
            )
            .lines(
                String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .take(12)
                    .map(|line| line.to_string()),
            )
            .hint(BUILD_WASM_HINT),
        );
        return report;
    }

    let mut measured: BTreeMap<(String, u32), Outcome> = BTreeMap::new();
    let mut unparsed: Vec<String> = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if !line.starts_with("line ") {
            if !line.trim().is_empty() {
                unparsed.push(line.to_string());
            }
            continue;
        }
        match parse_wasm_line(line) {
            Ok((config, seed, outcome)) => {
                measured.insert((config, seed), outcome);
            }
            Err(message) => unparsed.push(message),
        }
    }
    if measured.is_empty() {
        report.push(
            Check::cannot_run("wasm instance", "the Node harness printed no measurement")
                .lines(unparsed),
        );
        return report;
    }

    for cfg in dataset::CONFIGS {
        let mut fields: [Vec<String>; 4] = Default::default();
        let mut compared = 0usize;
        let mut missing: Vec<String> = Vec::new();
        for seed in PARITY_SEEDS.0..=PARITY_SEEDS.1 {
            let game = match engine::generate_game(seed, cfg.pieces, cfg.cap) {
                Ok(game) => game,
                Err(error) => {
                    missing.push(format!("seed {seed}: the Generator refused it ({error:?})"));
                    continue;
                }
            };
            let native = Outcome::from_native(&engine::solve(
                &game.board,
                game.sequence.as_slice(),
                &engine::SolveConfig::new(cfg.goal, GATE_BEAM),
                engine::shipped(),
            ));
            let Some(wasm) = measured.get(&(cfg.dir.to_string(), seed)) else {
                missing.push(format!("seed {seed}: the wasm side returned no line"));
                continue;
            };
            compared += 1;
            if native.status != wasm.status {
                fields[0].push(format!(
                    "seed {seed} · status · native {} · wasm {}",
                    native.status, wasm.status
                ));
            }
            if native.won != wasm.won {
                fields[1].push(format!(
                    "seed {seed} · won · native {} · wasm {}",
                    native.won as u8, wasm.won as u8
                ));
            }
            if native.evaluations != wasm.evaluations {
                fields[2].push(format!(
                    "seed {seed} · evaluations · native {} · wasm {}",
                    native.evaluations, wasm.evaluations
                ));
            }
            if native.lines != wasm.lines {
                fields[3].push(format!(
                    "seed {seed} · lines · native {} · wasm {}",
                    native.lines, wasm.lines
                ));
            }
        }

        // The move count is free (both sides already carry it) and is reported
        // rather than asserted: a Move list that differs in order but not in
        // count would be caught by gate 5, not here.
        let mut move_count_mismatch = Vec::new();
        for (key, wasm) in &measured {
            if key.0 != cfg.dir {
                continue;
            }
            if let Ok(game) = engine::generate_game(key.1, cfg.pieces, cfg.cap) {
                let native = Outcome::from_native(&engine::solve(
                    &game.board,
                    game.sequence.as_slice(),
                    &engine::SolveConfig::new(cfg.goal, GATE_BEAM),
                    engine::shipped(),
                ));
                if native.moves != wasm.moves {
                    move_count_mismatch.push(format!(
                        "seed {} · moves · native {} · wasm {}",
                        key.1, native.moves, wasm.moves
                    ));
                }
            }
        }

        for (index, field) in ["status", "won", "evaluations", "lines"].iter().enumerate() {
            let name = format!("{} · parity · {field}", cfg.dir);
            let failures = std::mem::take(&mut fields[index]);
            push_outcome(
                &mut report,
                name,
                failures,
                format!(
                    "{compared} seeds identical across {}-{}",
                    PARITY_SEEDS.0, PARITY_SEEDS.1
                ),
            );
        }
        push_outcome(
            &mut report,
            format!("{} · parity · moves", cfg.dir),
            move_count_mismatch,
            format!("{compared} seeds identical move counts"),
        );
        if !missing.is_empty() {
            push_outcome(
                &mut report,
                format!("{} · parity · seeds measured", cfg.dir),
                missing,
                String::new(),
            );
        }
    }
    report
}

/// `line config=goal8-p40-cap7 seed=0 won=1 status=0 evaluations=10212 lines=8 moves=20`
fn parse_wasm_line(line: &str) -> Result<(String, u32, Outcome), String> {
    let mut config: Option<String> = None;
    let mut seed: Option<u32> = None;
    let mut values: BTreeMap<&str, u32> = BTreeMap::new();
    for token in line.split_whitespace().skip(1) {
        let Some((key, value)) = token.split_once('=') else {
            return Err(format!(
                "gate-artifacts.mjs printed an unparseable token {token:?}"
            ));
        };
        match key {
            "config" => config = Some(value.to_string()),
            "seed" => {
                seed = Some(
                    value
                        .parse()
                        .map_err(|_| format!("gate-artifacts.mjs printed seed={value:?}"))?,
                )
            }
            other => {
                let parsed = value
                    .parse()
                    .map_err(|_| format!("gate-artifacts.mjs printed {other}={value:?}"))?;
                values.insert(other, parsed);
            }
        }
    }
    let config = config.ok_or_else(|| format!("gate-artifacts.mjs printed no config: {line:?}"))?;
    let seed = seed.ok_or_else(|| format!("gate-artifacts.mjs printed no seed: {line:?}"))?;
    let get = |key: &str| {
        values
            .get(key)
            .copied()
            .ok_or_else(|| format!("gate-artifacts.mjs printed no {key}=: {line:?}"))
    };
    Ok((
        config,
        seed,
        Outcome {
            status: get("status")? as u8,
            won: get("won")? != 0,
            evaluations: get("evaluations")?,
            lines: get("lines")?,
            moves: get("moves")?,
        },
    ))
}

fn node_binary() -> std::ffi::OsString {
    std::env::var_os("TETRIS_NODE").unwrap_or_else(|| "node".into())
}

// ------------------------------------------------- gate 5: determinism

/// §9.2 gate 5: the same Seed twice, in one process and across two.
fn determinism(options: &Options) -> Report {
    let mut report = Report::new(
        5,
        "determinism",
        format!(
            "seeds {:?} · beam {} · one process and two",
            DETERMINISM_SEEDS, GATE_BEAM
        ),
    );
    for cfg in dataset::CONFIGS {
        let mut failures: Vec<String> = Vec::new();
        for seed in DETERMINISM_SEEDS {
            let Ok(game) = engine::generate_game(seed, cfg.pieces, cfg.cap) else {
                failures.push(format!("seed {seed}: the Generator refused it"));
                continue;
            };
            let run = || {
                engine::solve(
                    &game.board,
                    game.sequence.as_slice(),
                    &engine::SolveConfig::new(cfg.goal, GATE_BEAM),
                    engine::shipped(),
                )
            };
            let first = run();
            let second = run();
            if first.status != second.status {
                failures.push(format!(
                    "seed {seed}: status {} then {}",
                    first.status.code(),
                    second.status.code()
                ));
            }
            if first.won != second.won {
                failures.push(format!(
                    "seed {seed}: won {} then {}",
                    first.won as u8, second.won as u8
                ));
            }
            if first.lines != second.lines {
                failures.push(format!(
                    "seed {seed}: {} then {} lines",
                    first.lines, second.lines
                ));
            }
            if first.evaluations != second.evaluations {
                failures.push(format!(
                    "seed {seed}: {} then {} evaluations",
                    first.evaluations, second.evaluations
                ));
            }
            if first.moves != second.moves {
                failures.push(format!(
                    "seed {seed}: the Move lists differ ({} words then {})",
                    first.moves.len(),
                    second.moves.len()
                ));
            }
            if first.packed() != second.packed() {
                failures.push(format!(
                    "seed {seed}: packed {} then {}",
                    first.packed(),
                    second.packed()
                ));
            }
        }
        push_outcome(
            &mut report,
            format!("{} · same seed in one process", cfg.dir),
            failures,
            format!(
                "{} seeds solved twice: status, won, lines, evaluations, moves and the packed i64 agree",
                DETERMINISM_SEEDS.len()
            ),
        );
    }

    match two_processes(&options.exe) {
        Ok(subject) => report.push(Check::pass("two processes", subject)),
        Err(CrossProcess::CannotRun(message)) => report.push(
            Check::cannot_run("two processes", message)
                .hint("gate 5 spawns itself (bench solve --emit) twice"),
        ),
        Err(CrossProcess::Differ {
            index,
            first,
            second,
        }) => report.push(
            Check::fail("two processes")
                .subject(format!("line {index} of the emitted output differs"))
                .lines([format!("run 1: {first}"), format!("run 2: {second}")]),
        ),
    }
    report
}

enum CrossProcess {
    CannotRun(String),
    Differ {
        index: usize,
        first: String,
        second: String,
    },
}

/// Two fresh address spaces, so the comparison catches a `HashMap` seed, an
/// uninitialised read or anything else that is stable within a process and not
/// between two. The unit of comparison is the emitted line, not the packed `i64`
/// alone: the packed value carries the Move *count*, not the Move order.
fn two_processes(exe: &Path) -> Result<String, CrossProcess> {
    if exe.as_os_str().is_empty() {
        return Err(CrossProcess::CannotRun(
            "the running executable's path is not known".to_string(),
        ));
    }
    let seeds = DETERMINISM_SEEDS
        .iter()
        .map(|seed| seed.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let emit = |run: u8| -> Result<Vec<u8>, CrossProcess> {
        let output = Command::new(exe)
            .arg("solve")
            .arg("--emit")
            .arg("--config")
            .arg("all")
            .arg("--seeds")
            .arg(&seeds)
            .arg("--beam")
            .arg(GATE_BEAM.to_string())
            .env("TETRIS_GATE_CHILD", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|error| {
                CrossProcess::CannotRun(format!(
                    "run {run}: cannot spawn {}: {error}",
                    exe.display()
                ))
            })?;
        if !output.status.success() {
            return Err(CrossProcess::CannotRun(format!(
                "run {run}: {} solve --emit exited with {:?}: {}",
                exe.display(),
                output.status.code(),
                String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .next()
                    .unwrap_or("")
            )));
        }
        Ok(output.stdout)
    };

    let first = emit(1)?;
    let second = emit(2)?;
    if first.is_empty() {
        return Err(CrossProcess::CannotRun(
            "solve --emit printed nothing".to_string(),
        ));
    }
    if first != second {
        let first_text = String::from_utf8_lossy(&first);
        let second_text = String::from_utf8_lossy(&second);
        let left: Vec<&str> = first_text.lines().collect();
        let right: Vec<&str> = second_text.lines().collect();
        for (index, (one, two)) in left.iter().zip(right.iter()).enumerate() {
            if one != two {
                return Err(CrossProcess::Differ {
                    index: index + 1,
                    first: (*one).to_string(),
                    second: (*two).to_string(),
                });
            }
        }
        return Err(CrossProcess::Differ {
            index: left.len().min(right.len()) + 1,
            first: format!("{} lines", left.len()),
            second: format!("{} lines", right.len()),
        });
    }
    Ok(format!(
        "{} lines byte-identical across two processes ({} bytes each)",
        String::from_utf8_lossy(&first).lines().count(),
        first.len()
    ))
}

// ----------------------------------------------------- gate 6: the budgets

/// §9.2 gate 6, first half: §9.3's two build-time budgets. The second half is
/// ticket 09's and is a fail-closed placeholder here.
fn budgets(options: &Options) -> Report {
    let paths = &options.paths;
    let mut report = Report::new(
        6,
        "budgets",
        format!(
            "wasm {} warn / {} fail · page {} fail gzip",
            WASM_WARN_BYTES, WASM_FAIL_BYTES, PAGE_FAIL_BYTES
        ),
    );

    if !paths.wasm.exists() {
        report.push(
            Check::cannot_run(
                "wasm raw size",
                format!("{} is missing", paths.wasm.display()),
            )
            .hint(BUILD_WASM_HINT),
        );
    } else {
        let size = size_of(&paths.wasm);
        let budget = format!("warn above {WASM_WARN_BYTES} B, fail above {WASM_FAIL_BYTES} B");
        if size > WASM_FAIL_BYTES {
            report.push(
                Check::fail("wasm raw size")
                    .subject(format!(
                        "{size} B raw is {} B over the {WASM_FAIL_BYTES} B §9.3 fail line",
                        size - WASM_FAIL_BYTES
                    ))
                    .lines([format!("budget: {budget}")])
                    .hint("§9.3 sizes the budget for the DT evaluator and the exact tail; a jump this large is a dependency, a build flag or debug info"),
            );
        } else if size > WASM_WARN_BYTES {
            report.push(Check::warn(
                "wasm raw size",
                format!("{size} B raw is over the {WASM_WARN_BYTES} B warn line; fail at {WASM_FAIL_BYTES} B"),
            ));
        } else {
            report.push(Check::pass(
                "wasm raw size",
                format!("{size} B raw, under the {WASM_WARN_BYTES} B warn line ({budget})"),
            ));
        }
    }

    page_gzip(&mut report, paths);

    for check in golden_checks(paths) {
        report.push(check);
    }

    report
}

/// Gate 6b: the exact fingerprint against the committed golden values, and the loose
/// clock bound around it (§9.2 gate 6, §9.3).
///
/// The order is the argument, not an implementation detail: the golden is loaded and
/// its identity checked *before* a single seed is compared, so a moved Dataset or a new
/// artifact can never be reported as a behaviour drift. The measurement itself is
/// [`crate::reference::measure`] — the same function `bench reference` appends a row
/// from — on the wasm substrate, because that is the engine a visitor's Run uses and
/// the golden's ms rows are wasm ones.
fn golden_checks(paths: &Paths) -> Vec<Check> {
    let path = paths.golden();
    if !path.exists() {
        return vec![Check::fail("golden evaluations")
            .subject(format!("no golden is committed at {}", path.display()))
            .lines([
                "gate 6b compares the evaluations-to-solve vector exactly; without a golden there is nothing to compare against".to_string(),
                format!(
                    "expected: 1000 seeds per configuration, width from {} (DEFAULT_BEAM), Split 0-999",
                    paths.measurements.join("width-curve.json").display()
                ),
            ])
            .hint("measure and append the Reference run first (`bench reference --append --reason \"…\"`), then pin the golden: `bench golden update --reason \"…\"`")];
    }
    let golden = match crate::golden::load(&path) {
        Ok(golden) => golden,
        Err(message) => {
            return vec![Check::fail("golden evaluations")
                .subject(message)
                .hint("the file is committed and must be readable; re-pin it with `bench golden update --reason \"…\"`")];
        }
    };

    // The identity: what the golden was pinned on. Cheap, and it comes first.
    let width = match crate::reference::width_from_curve(&paths.measurements.join("width-curve.json")) {
        Ok(width) => width,
        Err(message) => {
            return vec![Check::cannot_run("golden identity", message)
                .hint("DEFAULT_BEAM is a committed number (§9.5): `bench width-curve` writes it")];
        }
    };
    let datasets = match crate::reference::dataset_ids(&paths.data) {
        Ok(datasets) => datasets,
        Err(message) => {
            return vec![Check::cannot_run("golden identity", message)
                .hint("run `cargo run -p bench --release -- verify` and re-export the Dataset")];
        }
    };
    if !paths.wasm.exists() {
        return vec![Check::cannot_run(
            "golden identity",
            format!("{} is missing", paths.wasm.display()),
        )
        .hint(BUILD_WASM_HINT)];
    }
    let engine = crate::reference::engine_label(&paths.wasm);
    let machine = crate::reference::machine_label(crate::reference::Substrate::Wasm);
    let identity = crate::golden::identity(engine, machine, datasets, width);

    // Only after the identity holds is anything measured: a golden from another
    // Dataset is not gate 6b's question, and saying so costs nothing.
    let (diffs, _) = crate::golden::identity_diffs(&golden, &identity);
    if !diffs.is_empty() {
        return crate::golden::checks(&golden, &identity, &[]);
    }

    let mut measurements = Vec::new();
    for config in dataset::CONFIGS {
        let options = crate::reference::MeasureOptions {
            config,
            split: crate::reference::Split::TEST,
            width: Some(width),
            substrate: crate::reference::Substrate::Wasm,
            cal: crate::reference::CAL_DEFAULT,
            wasm: &paths.wasm,
        };
        match crate::reference::measure(&options) {
            Ok(measurement) => measurements.push(measurement),
            Err(message) => {
                return vec![Check::cannot_run(
                    format!("golden evaluations · {}", config.dir),
                    message,
                )
                .hint("Node 24 loads the artifact through the page's own loader; `npm --prefix web run build:wasm` builds it")];
            }
        }
    }
    crate::golden::checks(&golden, &identity, &measurements)
}

/// §9.3's whole-page transferred gzip: every hashed asset in `web/dist`, summed
/// in memory by Node's `gzipSync`. No network, no server, no `Content-Encoding`
/// assumption.
fn page_gzip(report: &mut Report, paths: &Paths) {
    if !paths.dist.is_dir() {
        report.push(
            Check::cannot_run("page gzip", format!("{} is missing", paths.dist.display()))
                .hint(BUILD_PAGE_HINT),
        );
        return;
    }
    if !paths.node_script.exists() {
        report.push(
            Check::cannot_run(
                "page gzip",
                format!("{} is missing", paths.node_script.display()),
            )
            .hint(BUILD_PAGE_HINT),
        );
        return;
    }
    let output = Command::new(node_binary())
        .arg(&paths.node_script)
        .arg("sizes")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            report.push(
                Check::cannot_run(
                    "page gzip",
                    format!("cannot spawn {}: {error}", node_binary().to_string_lossy()),
                )
                .hint("Node 24 is the runner for the gzip half of gate 6"),
            );
            return;
        }
    };
    if !output.status.success() {
        report.push(
            Check::cannot_run(
                "page gzip",
                format!(
                    "gate-artifacts.mjs sizes exited with {:?}",
                    output.status.code()
                ),
            )
            .lines(
                String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .take(12)
                    .map(|line| line.to_string()),
            )
            .hint(BUILD_PAGE_HINT),
        );
        return;
    }

    // `size total=24549 raw=59705 files=4`, plus one `file name=... raw=... gzip=...`
    // per asset so a regression is attributable to one file.
    let mut total: Option<u64> = None;
    let mut raw_total = 0u64;
    let mut files = 0usize;
    let mut by_kind: BTreeMap<String, u64> = BTreeMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut tokens = line.split_whitespace();
        match tokens.next() {
            Some("size") => {
                for token in tokens {
                    let Some((key, value)) = token.split_once('=') else {
                        continue;
                    };
                    let Ok(value) = value.parse::<u64>() else {
                        continue;
                    };
                    match key {
                        "total" => total = Some(value),
                        "raw" => raw_total = value,
                        "files" => files = value as usize,
                        _ => {}
                    }
                }
            }
            Some("file") => {
                let mut name = String::new();
                let mut gzip = 0u64;
                for token in tokens {
                    let Some((key, value)) = token.split_once('=') else {
                        continue;
                    };
                    match key {
                        "name" => name = value.to_string(),
                        "gzip" => gzip = value.parse().unwrap_or(0),
                        _ => {}
                    }
                }
                let kind = name.rsplit('.').next().unwrap_or("other").to_string();
                *by_kind.entry(kind).or_insert(0) += gzip;
            }
            _ => {}
        }
    }

    let Some(total) = total else {
        report.push(
            Check::cannot_run("page gzip", "the Node harness printed no total")
                .hint(BUILD_PAGE_HINT),
        );
        return;
    };
    let components = ["wasm", "js", "css", "html"]
        .iter()
        .filter_map(|kind| by_kind.get(*kind).map(|bytes| format!("{kind} {bytes}")))
        .collect::<Vec<_>>()
        .join(" + ");
    if total > PAGE_FAIL_BYTES {
        report.push(
            Check::fail("page gzip")
                .subject(format!(
                    "{total} B gzip is {} B over the {PAGE_FAIL_BYTES} B §9.3 fail line",
                    total - PAGE_FAIL_BYTES
                ))
                .lines([format!(
                    "{files} hashed assets, {raw_total} B raw: {components}"
                )])
                .hint(BUILD_PAGE_HINT),
        );
    } else {
        report.push(Check::pass(
            "page gzip",
            format!(
                "{total} B gzip over {files} hashed assets ({components}) from {raw_total} B raw, under {PAGE_FAIL_BYTES} B"
            ),
        ));
    }
}
