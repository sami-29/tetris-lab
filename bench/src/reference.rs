//! §9.6: the Reference run, as a row a visitor can be compared against.
//!
//! One measurement per configuration: every solve of the test Split at
//! `DEFAULT_BEAM`, the six statistics §9.6 names, and the label block that makes the
//! numbers mean something — engine commit, wasm hash, build flags, CPU, OS, runtime,
//! date.
//!
//! Three decisions are load-bearing, and each is the ticket's rather than this
//! file's:
//!
//! * **The substrate is wasm.** The Run a visitor triggers solves the artifact in
//!   `web/pkg/engine.wasm`, so the Reference rows it is compared against are
//!   measured on the same artifact through Node's V8 — the engine headless Chrome
//!   runs, without the DOM. Evaluations and Lines are substrate-independent (both
//!   substrates produce identical integers, notes/09 §0.3), but milliseconds are
//!   not, so no row mixes the two: a native row says `native` and is not what the
//!   page compares against.
//! * **`null` is not `0`.** A saturated bed has no lines-on-failure samples, and a
//!   stored `0` would make the page print a measurement that does not exist.
//! * **The file is append-only.** [`append`] is the only writer, it opens with
//!   `append(true)`, and it refuses a row whose `index` is not the next one. A
//!   re-measurement adds a dated row with its reason; nothing is ever rewritten.
//!
//! The statistics are exactly notes/09 §1.2's: the Wilson interval without a
//! continuity correction, nearest-rank percentiles (shared with [`crate::width`], so
//! there is one definition of p90), and FNV-1a-64 over the per-seed vectors so the
//! golden compare (§9.2 gate 6) is a 16-hex eyeball before it is a row walk.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::dataset::{self, Config};
use crate::fingerprint::{self, Fnv};
use crate::json::{self, Value};
use crate::sha256;
use crate::width;

// ------------------------------------------------------------- the constants

/// The row's format. A reader that does not know it must refuse the row rather than
/// guess at its fields.
pub const FORMAT: u32 = 1;
/// The row's `kind`: a log line, not an artifact.
pub const KIND: &str = "reference";

/// §9.4's shipped Evaluator, as the row names it. `bench/tests/reference.rs` holds
/// this against `data/measurements/ablation-ladder.json`'s `shipped` block, because
/// ticket 08's ladder is the authority for what ships.
pub const EVALUATOR_ID: &str = "E1";
/// §9.4's shipped weight table.
pub const EVALUATOR_TABLE: &str = "E1_PUBLISHED";
/// The shipped search decisions: the Tail on, and everything else unconditional
/// (ticket 08 deleted the Dedup and No-suicide off-switches from the engine).
pub const TOGGLES: &str = "shipped";

/// The build flags, as a label. `Cargo.toml`'s `[profile.release]` is the source and
/// a test compares the two, so this string cannot drift from the flags that built
/// the numbers.
pub const ENGINE_PROFILE: &str = "release: lto=fat, codegen-units=1, panic=abort, strip=true; \
     target wasm32-unknown-unknown; no wasm-bindgen, no wasm-opt";

/// The file inside `data/measurements/` that `DEFAULT_BEAM` and the Width cap live
/// in (§9.5). The Reference run reads it; it never takes the width on trust.
pub const WIDTH_CURVE_FILE: &str = "width-curve.json";
/// The append-only log inside each configuration's directory.
pub const REFERENCE_FILE: &str = "reference.jsonl";
/// The committed golden fingerprint (§9.2 gate 6).
pub const GOLDEN_FILE: &str = "golden-evals.json";

/// The calibration slice's repeats. Ten solves of a fixed game cost ~0.4 s and buy a
/// bound that survives a shared box (notes/09 §4.6).
pub const CAL_DEFAULT: u32 = 10;
/// The calibration game: the harder configuration at width 200 — big enough that a
/// loaded box's slowdown shows, cheap enough that ten repeats are noise-free.
pub const CAL_GOAL: u32 = 12;
pub const CAL_PIECES: u32 = 100;
pub const CAL_CAP: u32 = 7;
pub const CAL_WIDTH: u32 = 200;

/// The two percentiles the row reports, and their method's name.
pub const P50_METHOD: &str = "nearest-rank";
pub const EVALS_METHOD: &str = "fnv1a64-over-le-u32-seed-order";
pub const LINES_METHOD: &str = "fnv1a64-over-u8-seed-order";

// ------------------------------------------------------------------ the shape

/// A seed range: §9.6's "the split and seed range".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Split {
    pub first: u32,
    pub last: u32,
}

impl Split {
    /// §6's test split.
    pub const TEST: Split = Split {
        first: dataset::TEST_SPLIT.0,
        last: dataset::TEST_SPLIT.1,
    };
    /// §6's tuning split: never a Reference run's split.
    pub const TUNING: Split = Split {
        first: dataset::TUNING_SPLIT.0,
        last: dataset::TUNING_SPLIT.1,
    };

    pub fn n(&self) -> u32 {
        self.last + 1 - self.first
    }

    pub fn label(&self) -> String {
        format!("{}-{}", self.first, self.last)
    }

    pub fn from_text(text: &str) -> Result<Split, String> {
        match text {
            "test" => Ok(Split::TEST),
            "tuning" => Ok(Split::TUNING),
            other => Err(format!(
                "--split must be test or tuning, found {other:?} (the Reference run is §9.6's test Split)"
            )),
        }
    }
}

/// Which engine produced the numbers. `wasm` means the artifact the page loads,
/// through Node's V8; `native` means the native bench build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Substrate {
    Native,
    Wasm,
}

impl Substrate {
    pub fn as_str(self) -> &'static str {
        match self {
            Substrate::Native => "native",
            Substrate::Wasm => "wasm",
        }
    }

    pub fn from_text(text: &str) -> Result<Substrate, String> {
        match text {
            "native" => Ok(Substrate::Native),
            "wasm" => Ok(Substrate::Wasm),
            other => Err(format!("substrate must be native or wasm, found {other:?}")),
        }
    }
}

/// The Dataset a row was measured on (§6.1). Two rows with one fingerprint were
/// measured on the same games, which is what makes their quality rows comparable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DatasetId {
    pub fingerprint: String,
    pub generator_version: u32,
}

/// What the search was asked: the behaviour half of the label block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Behaviour {
    pub goal: u32,
    pub pieces: u32,
    pub cap: u32,
    pub width: u32,
    /// `artifact` when the width came from `width-curve.json`, `flag` when it was
    /// overridden — an override must never masquerade as the shipped default (§9.5).
    pub width_source: String,
    pub evaluator: String,
    pub table: String,
    pub table_fnv: String,
    pub toggles: String,
    pub tail: bool,
    pub tail_pieces: usize,
}

/// The engine's identity: what was built, from which commit, and what the artifact
/// hashes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Engine {
    pub commit: String,
    pub short: String,
    pub dirty: bool,
    pub wasm_sha256: String,
    pub wasm_bytes: u64,
    pub profile: String,
    pub toolchain: String,
}

/// The box and the runtime: §2.4's "CPU, OS, runtime, browser/Node", and the date's
/// neighbour `measured_at`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Machine {
    pub cpu: String,
    pub os: String,
    pub runtime: String,
    pub substrate: String,
    pub browser_or_node: String,
    pub rustc: String,
    pub node: String,
}

/// One solve, as the row's evidence: the vector a golden is compared against.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub seed: u32,
    pub won: bool,
    pub lines: u32,
    pub evaluations: u32,
    pub ms: f64,
}

/// §9.6's six statistics plus the two clocks the 4x bound is normalised by.
#[derive(Clone, Debug, PartialEq)]
pub struct Stats {
    pub n: u32,
    pub wins: u32,
    pub n_fail: u32,
    pub win_rate: f64,
    pub wilson95: [f64; 2],
    pub fail_lines_mean: Option<f64>,
    pub fail_lines_median: Option<u32>,
    pub evals_p50: u32,
    pub evals_p90: u32,
    pub evals_mean: f64,
    pub evals_max: u32,
    pub ms_p50: f64,
    pub ms_p90: f64,
    pub ms_max: f64,
    /// One clock across the whole Split, generation and bookkeeping included: the
    /// wall clock a visitor's Run would be compared to.
    pub batch_ms: f64,
    /// The mean of the two calibration slices: the machine-speed normaliser (§4.6).
    pub cal_ms: f64,
    pub cal_ms_before: f64,
    pub cal_ms_after: f64,
}

/// The quality half of a row: §9.6's "Quality rows (win rate, lines, evaluations)".
/// Held exact for one identity; the machine rows beside it may differ.
#[derive(Clone, Debug, PartialEq)]
pub struct Quality {
    pub wins: u32,
    pub n_fail: u32,
    pub fail_lines_mean: Option<f64>,
    pub fail_lines_median: Option<u32>,
    pub evals_p50: u32,
    pub evals_p90: u32,
    pub evals_max: u32,
}

impl Stats {
    pub fn quality(&self) -> Quality {
        Quality {
            wins: self.wins,
            n_fail: self.n_fail,
            fail_lines_mean: self.fail_lines_mean,
            fail_lines_median: self.fail_lines_median,
            evals_p50: self.evals_p50,
            evals_p90: self.evals_p90,
            evals_max: self.evals_max,
        }
    }
}

/// The two per-seed digests, and the method names that make them reproducible: a
/// digest without its recipe is a number nobody can re-derive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fingerprints {
    pub evals_fnv1a64: String,
    pub lines_fnv1a64: String,
    pub evals_method: String,
    pub lines_method: String,
    pub p90_method: String,
}

/// One measurement of one configuration over one split. What `bench reference`
/// produces and what the golden compares against.
#[derive(Clone, Debug, PartialEq)]
pub struct Measurement {
    pub config: Config,
    pub width: u32,
    pub split: Split,
    pub samples: Vec<Sample>,
    pub batch_ms: f64,
    pub cal_ms_before: f64,
    pub cal_ms_after: f64,
}

impl Measurement {
    /// The evaluations-to-solve vector, in seed order.
    pub fn evals(&self) -> Vec<u32> {
        self.samples
            .iter()
            .map(|sample| sample.evaluations)
            .collect()
    }

    /// The lines-to-solve vector, in seed order.
    pub fn lines(&self) -> Vec<u32> {
        self.samples.iter().map(|sample| sample.lines).collect()
    }

    /// §1.2's statistics, computed once, from the samples in seed order.
    pub fn stats(&self) -> Stats {
        let n = self.samples.len() as u32;
        let wins = self.samples.iter().filter(|sample| sample.won).count() as u32;
        let n_fail = n - wins;

        let mut fail_lines: Vec<u32> = self
            .samples
            .iter()
            .filter(|sample| !sample.won)
            .map(|sample| sample.lines)
            .collect();
        fail_lines.sort_unstable();
        let fail_lines_mean = if fail_lines.is_empty() {
            None
        } else {
            let sum: u64 = fail_lines.iter().map(|lines| *lines as u64).sum();
            Some(sum as f64 / fail_lines.len() as f64)
        };
        let fail_lines_median = median_u32(&fail_lines);

        let evals = self.evals();
        let mut sorted_evals: Vec<f64> = evals.iter().map(|value| *value as f64).collect();
        sorted_evals.sort_by(|a, b| a.partial_cmp(b).expect("counts are never NaN"));
        let evals_max = sorted_evals.last().copied().unwrap_or(0.0) as u32;
        let evals_mean = if sorted_evals.is_empty() {
            0.0
        } else {
            sorted_evals.iter().sum::<f64>() / sorted_evals.len() as f64
        };

        let mut ms: Vec<f64> = self.samples.iter().map(|sample| sample.ms).collect();
        ms.sort_by(|a, b| a.partial_cmp(b).expect("durations are never NaN"));

        Stats {
            n,
            wins,
            n_fail,
            win_rate: if n == 0 { 0.0 } else { wins as f64 / n as f64 },
            wilson95: wilson95(wins, n),
            fail_lines_mean,
            fail_lines_median,
            evals_p50: width::nearest_rank(&sorted_evals, 1, 2) as u32,
            evals_p90: width::nearest_rank(&sorted_evals, 9, 10) as u32,
            evals_mean,
            evals_max,
            ms_p50: width::nearest_rank(&ms, 1, 2),
            ms_p90: width::nearest_rank(&ms, 9, 10),
            ms_max: ms.last().copied().unwrap_or(0.0),
            batch_ms: self.batch_ms,
            cal_ms: (self.cal_ms_before + self.cal_ms_after) / 2.0,
            cal_ms_before: self.cal_ms_before,
            cal_ms_after: self.cal_ms_after,
        }
    }

    /// §1.2: FNV-1a-64 over the evaluations, 4 bytes little-endian each, seed order.
    pub fn evals_fingerprint(&self) -> String {
        let mut hash = Fnv::new();
        for value in self.evals() {
            hash.u32_le(value);
        }
        fingerprint::hex(hash.finish())
    }

    /// §1.2: FNV-1a-64 over the lines, one byte each, seed order.
    pub fn lines_fingerprint(&self) -> String {
        let mut hash = Fnv::new();
        for value in self.lines() {
            hash.byte(value as u8);
        }
        fingerprint::hex(hash.finish())
    }

    pub fn fingerprints(&self) -> Fingerprints {
        Fingerprints {
            evals_fnv1a64: self.evals_fingerprint(),
            lines_fnv1a64: self.lines_fingerprint(),
            evals_method: EVALS_METHOD.to_string(),
            lines_method: LINES_METHOD.to_string(),
            p90_method: P50_METHOD.to_string(),
        }
    }

    /// A measurement whose samples carry the given vectors and no clock. This is how
    /// the golden's comparisons are tested without solving a thousand games, and how
    /// a committed golden is read back into the same structure a measured run
    /// produces — one comparison path, never two.
    pub fn from_vectors(
        config: Config,
        width: u32,
        split: Split,
        evals: &[u32],
        lines: &[u32],
        batch_ms: f64,
        cal_ms: f64,
    ) -> Measurement {
        let samples = (0..evals.len().min(lines.len()))
            .map(|index| Sample {
                seed: split.first + index as u32,
                won: lines[index] >= config.goal,
                lines: lines[index],
                evaluations: evals[index],
                ms: 0.0,
            })
            .collect();
        Measurement {
            config,
            width,
            split,
            samples,
            batch_ms,
            cal_ms_before: cal_ms,
            cal_ms_after: cal_ms,
        }
    }
}

/// Wilson's score interval for a proportion, 95 %, no continuity correction: the
/// formula notes/09 §1.2 fixes, including its exactness at the two ends (a
/// saturated bed is exactly 1 at the top, an empty one exactly 0 at the bottom —
/// the formula gives those in real arithmetic, and floating point leaves 2e-19
/// behind, which is not a number the Reference row should print).
pub fn wilson95(wins: u32, n: u32) -> [f64; 2] {
    const Z: f64 = 1.959_963_984_540_054;
    if n == 0 {
        return [0.0, 0.0];
    }
    let n = n as f64;
    let p = wins as f64 / n;
    let denominator = 1.0 + Z * Z / n;
    let centre = (p + Z * Z / (2.0 * n)) / denominator;
    let half = (Z / denominator) * (p * (1.0 - p) / n + Z * Z / (4.0 * n * n)).sqrt();
    let mut lower = (centre - half).clamp(0.0, 1.0);
    let mut upper = (centre + half).clamp(0.0, 1.0);
    // The two ends are exact in real arithmetic (p = 0 gives 0, p = 1 gives 1), but
    // `centre ± half` lands 2e-19 away from the boundary in f64. Pin them: a Reference
    // row prints 0 and 1, not 2.17e-19.
    if wins == 0 {
        lower = 0.0;
    }
    if wins as f64 >= n {
        upper = 1.0;
    }
    [lower, upper]
}

/// Nearest rank on a sorted slice, `k = ceil(p * n)`; `None` for no samples. `null`
/// rather than 0, for the same reason the lines rows are `null` (§9.6).
fn median_u32(sorted: &[u32]) -> Option<u32> {
    if sorted.is_empty() {
        return None;
    }
    let rank = sorted.len().div_ceil(2); // ceil(0.5 * n)
    Some(sorted[rank - 1])
}

// -------------------------------------------------------------- the label block

/// The wasm artifact the page loads, relative to the workspace root.
pub const WASM_ARTIFACT: &str = "web/pkg/engine.wasm";

/// The workspace root, from this crate's own directory: the Gates and the
/// Reference run behave the same from anywhere.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("bench/ always has a parent: the workspace root")
        .to_path_buf()
}

/// `data/measurements`.
pub fn measurements_dir() -> PathBuf {
    root().join("data").join("measurements")
}

/// `data/measurements/width-curve.json`.
pub fn width_curve_path() -> PathBuf {
    measurements_dir().join(WIDTH_CURVE_FILE)
}

/// `data/measurements/golden-evals.json`.
pub fn golden_path() -> PathBuf {
    measurements_dir().join(GOLDEN_FILE)
}

/// The width and where it came from: `width-curve.json`'s `default_beam`, or a flag.
pub fn width_from_curve(path: &Path) -> Result<u32, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("read {}: {error}", path.display()))?;
    let value = json::parse(&text).map_err(|error| format!("parse {}: {error}", path.display()))?;
    value
        .get("default_beam")
        .and_then(Value::as_u32)
        .ok_or_else(|| {
            format!(
                "{}: default_beam is missing or not a positive integer; §9.5 makes it a committed \
                 number, so the Reference run refuses to invent one",
                path.display()
            )
        })
}

/// The Evaluator's table fingerprint (FNV-1a-64 over the eight weights, notes/07
/// §4.2), taken from the shipped table itself rather than typed out here.
pub fn table_fnv() -> String {
    crate::tuning::table_fnv(&engine::E1_PUBLISHED.as_array())
}

/// §6.1's Dataset identity, *re-derived*: every game regenerated from its seed and
/// the fingerprint recomputed, then compared to the manifest. A Reference run that
/// skipped this could be measured on games that are not the committed ones.
pub fn dataset_id(data: &Path, config: &Config) -> Result<DatasetId, String> {
    let path = dataset::manifest_path(data, config);
    let text = fs::read_to_string(&path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let manifest = dataset::Manifest::parse(&text).map_err(|error| format!("{}: {error}", path.display()))?;

    let mut games = Vec::with_capacity(dataset::GAME_COUNT);
    for seed in dataset::SEED_FIRST..=dataset::SEED_LAST {
        let game = engine::generate_game(seed, config.pieces, config.cap)
            .map_err(|error| format!("generate seed {seed}: {error:?}"))?;
        games.push(dataset::GameLine::from_engine(&game));
    }
    let fingerprint = fingerprint::hex(fingerprint::fingerprint(&games));
    if fingerprint != manifest.fingerprint {
        return Err(format!(
            "{}: the regenerated games fingerprint to {fingerprint}, the manifest says {} — the \
             Dataset and the generator disagree (run `bench verify`)",
            config.dir, manifest.fingerprint
        ));
    }
    Ok(DatasetId {
        fingerprint,
        generator_version: manifest.generator_version,
    })
}

/// Both configurations' identities, in `dataset::CONFIGS` order.
pub fn dataset_ids(data: &Path) -> Result<Vec<(String, DatasetId)>, String> {
    dataset::CONFIGS
        .iter()
        .map(|config| Ok((config.dir.to_string(), dataset_id(data, config)?)))
        .collect()
}

/// The value `--wasm` or the default points at, hashed: the engine's identity.
pub fn engine_label(wasm: &Path) -> Engine {
    let (wasm_sha256, wasm_bytes) = match sha256::file(wasm) {
        Ok((hash, bytes)) => (hash, bytes),
        Err(_) => (String::new(), 0),
    };
    let commit = git(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown (git rev-parse failed)".to_string());
    let short = git(&["rev-parse", "--short", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
    let dirty = dirty();
    Engine {
        commit,
        short,
        dirty,
        wasm_sha256,
        wasm_bytes,
        profile: ENGINE_PROFILE.to_string(),
        toolchain: toolchain(),
    }
}

/// The box, the runtime and the substrate: probes, not memory. A probe that fails
/// says so — an invented CPU name is worse than an honest "unknown".
pub fn machine_label(substrate: Substrate) -> Machine {
    let node = probe("node", &["--version"]).unwrap_or_else(|| "node (version unknown)".to_string());
    let rustc = probe("rustc", &["-V"]).unwrap_or_else(|| "rustc (version unknown)".to_string());
    let host = probe("rustc", &["-vV"])
        .and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix("host: ").map(|host| host.trim().to_string()))
        })
        .unwrap_or_else(|| format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS));

    let (cpu, os) = hardware().unwrap_or_else(|| {
        (
            format!(
                "unknown CPU ({} logical cores)",
                std::thread::available_parallelism()
                    .map(|count| count.get())
                    .unwrap_or(0)
            ),
            format!("{} (no powershell probe)", std::env::consts::OS),
        )
    });

    let runtime = match substrate {
        Substrate::Wasm => format!(
            "{node} (V8 headless, not a browser: the engine headless Chrome runs, without the DOM)"
        ),
        Substrate::Native => {
            let short = rustc
                .split_whitespace()
                .nth(1)
                .unwrap_or("unknown")
                .to_string();
            format!("native bench ({host}, rustc {short})")
        }
    };
    Machine {
        cpu,
        os,
        runtime,
        substrate: substrate.as_str().to_string(),
        browser_or_node: match substrate {
            Substrate::Wasm => format!("{node} (V8 headless), not a browser"),
            Substrate::Native => "none: native harness, not a browser (§9.6, ADR 0002)".to_string(),
        },
        rustc,
        node,
    }
}

fn probe(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
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

/// `git` against the workspace root.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root())
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

/// Whether the tree the engine was built from is dirty. Scoped to what can move the
/// numbers: a scratch file in `.scratch/` is not a behaviour change, but an
/// uncommitted `engine/src/search.rs` is.
fn dirty() -> bool {
    let output = Command::new("git")
        .args([
            "status",
            "--porcelain",
            "--",
            "engine",
            "engine-wasm",
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
        ])
        .current_dir(root())
        .output();
    match output {
        Ok(output) if output.status.success() => !String::from_utf8_lossy(&output.stdout).trim().is_empty(),
        _ => false,
    }
}

/// §10's pinned toolchain's own answer, from `rustc -vV`'s release and host.
fn toolchain() -> String {
    let release = probe("rustc", &["-V"])
        .and_then(|text| text.split_whitespace().nth(1).map(|version| version.to_string()))
        .unwrap_or_else(|| "unknown".to_string());
    let host = probe("rustc", &["-vV"])
        .and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix("host: ").map(|host| host.trim().to_string()))
        })
        .unwrap_or_else(|| format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS));
    format!("{release}-{host}")
}

/// CPU and OS, asked of the operating system. One `powershell` call because Windows
/// is the build box (`run/log.md`: the pinned host toolchain needs a GNU linker);
/// anywhere else the caller's fallback is used and the label says so.
fn hardware() -> Option<(String, String)> {
    let script = "$c=Get-CimInstance Win32_Processor|Select-Object -First 1;\
                  $s=Get-CimInstance Win32_ComputerSystem;\
                  $o=Get-CimInstance Win32_OperatingSystem;\
                  '{0}|{1}|{2}|{3}|{4}|{5}' -f $c.Name.Trim(),$c.NumberOfCores,$c.NumberOfLogicalProcessors,\
                  [math]::Round($s.TotalPhysicalMemory/1GB,1),$o.Caption.Trim(),$o.Version.Trim()";
    let text = probe_powershell(script)?;
    let fields: Vec<&str> = text.split('|').collect();
    if fields.len() != 6 {
        return None;
    }
    Some((
        format!(
            "{} ({}C/{}T), {} GB",
            fields[0], fields[1], fields[2], fields[3]
        ),
        format!("{} {}", fields[4], fields[5]),
    ))
}

fn probe_powershell(script: &str) -> Option<String> {
    let output = Command::new("powershell")
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

/// ISO-8601 UTC, seconds, `Z`.
pub fn now_iso() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    width::iso_utc(seconds)
}

// --------------------------------------------------------------- the measuring

/// What one measurement needs.
pub struct MeasureOptions<'a> {
    pub config: Config,
    pub split: Split,
    /// `None` reads `width-curve.json`; `Some` is a flag, and says so in the row.
    pub width: Option<u32>,
    pub substrate: Substrate,
    pub cal: u32,
    pub wasm: &'a Path,
}

/// Solve every seed of the split, in seed order, and time it.
///
/// Generation is *inside* the `batch_ms` clock and outside the per-solve clock: the
/// batch number is the wall clock a visitor's Run pays, and the per-solve number is
/// the search.
pub fn measure(options: &MeasureOptions<'_>) -> Result<Measurement, String> {
    let width = match options.width {
        Some(width) => width,
        None => width_from_curve(&width_curve_path())?,
    };
    match options.substrate {
        Substrate::Native => measure_native(options, width),
        Substrate::Wasm => measure_wasm(options, width),
    }
}

fn measure_native(options: &MeasureOptions<'_>, width: u32) -> Result<Measurement, String> {
    let config = options.config;
    let split = options.split;

    // A warm-up that is not a sample: the first board of every shape pays for
    // page faults and branch learning, and that is not the search's cost.
    for seed in split.first..(split.first + 2).min(split.last + 1) {
        solve_native(seed, config, width)?;
    }

    let cal_ms_before = calibrate_native(options.cal)?;
    let mut samples = Vec::with_capacity(split.n() as usize);
    let started = Instant::now();
    for seed in split.first..=split.last {
        let game = engine::generate_game(seed, config.pieces, config.cap)
            .map_err(|error| format!("generate seed {seed}: {error:?}"))?;
        let at = Instant::now();
        let result = engine::solve(
            &game.board,
            game.sequence.as_slice(),
            &engine::SolveConfig::new(config.goal, width),
            engine::shipped(),
        );
        let ms = at.elapsed().as_secs_f64() * 1000.0;
        samples.push(Sample {
            seed,
            won: result.won,
            lines: result.lines,
            evaluations: result.evaluations,
            ms,
        });
    }
    let batch_ms = started.elapsed().as_secs_f64() * 1000.0;
    let cal_ms_after = calibrate_native(options.cal)?;

    Ok(Measurement {
        config,
        width,
        split,
        samples,
        batch_ms,
        cal_ms_before,
        cal_ms_after,
    })
}

fn solve_native(seed: u32, config: Config, width: u32) -> Result<(), String> {
    let game = engine::generate_game(seed, config.pieces, config.cap)
        .map_err(|error| format!("generate seed {seed}: {error:?}"))?;
    let _ = engine::solve(
        &game.board,
        game.sequence.as_slice(),
        &engine::SolveConfig::new(config.goal, width),
        engine::shipped(),
    );
    Ok(())
}

/// The calibration slice: the same fixed game solved `cal` times, timed. Its only
/// job is to measure how fast *this process on this box* is right now, which is what
/// the 4x bound is divided by.
pub fn calibrate_native(cal: u32) -> Result<f64, String> {
    if cal == 0 {
        return Ok(0.0);
    }
    let game = engine::generate_game(0, CAL_PIECES, CAL_CAP)
        .map_err(|error| format!("generate the calibration game: {error:?}"))?;
    let mut total = Duration::ZERO;
    for _ in 0..cal {
        let at = Instant::now();
        let _ = engine::solve(
            &game.board,
            game.sequence.as_slice(),
            &engine::SolveConfig::new(CAL_GOAL, CAL_WIDTH),
            engine::shipped(),
        );
        total += at.elapsed();
    }
    Ok(total.as_secs_f64() * 1000.0)
}

/// `bench/tools/reference-wasm.mjs`: the wasm half, through the page's own loader.
pub fn wasm_script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tools")
        .join("reference-wasm.mjs")
}

fn measure_wasm(options: &MeasureOptions<'_>, width: u32) -> Result<Measurement, String> {
    let script = wasm_script();
    if !script.exists() {
        return Err(format!("missing {}", script.display()));
    }
    if !options.wasm.exists() {
        return Err(format!(
            "{} is missing; build it with `npm --prefix web run build:wasm`",
            options.wasm.display()
        ));
    }
    let output = Command::new("node")
        .arg(&script)
        .arg("--wasm")
        .arg(options.wasm)
        .arg("--goal")
        .arg(options.config.goal.to_string())
        .arg("--pieces")
        .arg(options.config.pieces.to_string())
        .arg("--cap")
        .arg(options.config.cap.to_string())
        .arg("--width")
        .arg(width.to_string())
        .arg("--first")
        .arg(options.split.first.to_string())
        .arg("--last")
        .arg(options.split.last.to_string())
        .arg("--cal")
        .arg(options.cal.to_string())
        .output()
        .map_err(|error| format!("run {}: {error}", script.display()))?;
    if !output.status.success() {
        return Err(format!(
            "{} failed: {}",
            script.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_wasm_rows(&String::from_utf8_lossy(&output.stdout), options, width)
}

/// The script's protocol, one record per line:
///
/// ```text
/// sample,<seed>,<won>,<lines>,<evaluations>,<ms>
/// cal_before,<ms>
/// cal_after,<ms>
/// batch_ms,<ms>
/// ```
fn parse_wasm_rows(text: &str, options: &MeasureOptions<'_>, width: u32) -> Result<Measurement, String> {
    let mut samples = Vec::new();
    let mut cal_before = Vec::new();
    let mut cal_after = Vec::new();
    let mut batch_ms = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split(',').collect();
        let number = |index: usize, what: &str| -> Result<f64, String> {
            fields
                .get(index)
                .ok_or_else(|| format!("reference-wasm: {line:?} has no {what}"))?
                .parse::<f64>()
                .map_err(|_| format!("reference-wasm: {what} {:?} in {line:?}", fields[index]))
        };
        match fields[0] {
            "sample" => {
                if fields.len() != 6 {
                    return Err(format!("reference-wasm: unexpected row {line:?}"));
                }
                let seed: u32 = fields[1]
                    .parse()
                    .map_err(|_| format!("reference-wasm: seed {:?}", fields[1]))?;
                let won = match fields[2] {
                    "1" | "true" => true,
                    "0" | "false" => false,
                    other => return Err(format!("reference-wasm: won {other:?}")),
                };
                let lines: u32 = fields[3]
                    .parse()
                    .map_err(|_| format!("reference-wasm: lines {:?}", fields[3]))?;
                let evaluations: u32 = fields[4]
                    .parse()
                    .map_err(|_| format!("reference-wasm: evaluations {:?}", fields[4]))?;
                let ms: f64 = fields[5]
                    .parse()
                    .map_err(|_| format!("reference-wasm: ms {:?}", fields[5]))?;
                samples.push(Sample {
                    seed,
                    won,
                    lines,
                    evaluations,
                    ms,
                });
            }
            "cal_before" => cal_before.push(number(1, "ms")?),
            "cal_after" => cal_after.push(number(1, "ms")?),
            "batch_ms" => batch_ms = Some(number(1, "ms")?),
            other => return Err(format!("reference-wasm: unknown record {other:?}")),
        }
    }

    let expected = options.split.n() as usize;
    if samples.len() != expected {
        return Err(format!(
            "reference-wasm: {} samples for {} seeds ({}–{})",
            samples.len(),
            expected,
            options.split.first,
            options.split.last
        ));
    }
    for (index, sample) in samples.iter().enumerate() {
        let expected_seed = options.split.first + index as u32;
        if sample.seed != expected_seed {
            return Err(format!(
                "reference-wasm: sample {index} is seed {}, expected {expected_seed} (seed order)",
                sample.seed
            ));
        }
    }
    let batch_ms = batch_ms.ok_or("reference-wasm: no batch_ms record")?;
    Ok(Measurement {
        config: options.config,
        width,
        split: options.split,
        samples,
        batch_ms,
        cal_ms_before: cal_before.iter().sum(),
        cal_ms_after: cal_after.iter().sum(),
    })
}

// ------------------------------------------------------------------ the row

/// What labels a run: the engine, the box, the Dataset identities, the date, the
/// reason, and where in the append-only chain these rows go.
#[derive(Clone, Debug)]
pub struct RunLabels {
    pub engine: Engine,
    pub machine: Machine,
    pub datasets: Vec<(String, DatasetId)>,
    pub measured_at: String,
    pub reason: String,
    pub first_index: u32,
    pub first_supersedes: Option<String>,
}

impl RunLabels {
    pub fn dataset(&self, config: &Config) -> Option<&DatasetId> {
        self.datasets
            .iter()
            .find(|(name, _)| name == config.dir)
            .map(|(_, id)| id)
    }
}

/// One line of `reference.jsonl`: §9.6's row.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub config: Config,
    pub index: u32,
    pub run_id: String,
    pub measured_at: String,
    pub reason: String,
    pub supersedes: Option<String>,
    pub dataset: DatasetId,
    pub behaviour: Behaviour,
    pub split: Split,
    pub stats: Stats,
    pub fingerprints: Fingerprints,
    pub engine: Engine,
    pub machine: Machine,
}

impl Row {
    pub fn quality(&self) -> Quality {
        self.stats.quality()
    }

    /// One compact JSON object, in a fixed key order, without the trailing newline
    /// (the writer owns that). One row per line is what makes `git diff` and
    /// `git log -p` reviewable.
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(1024);
        out.push('{');
        out.push_str(&format!("\"format\":{FORMAT},"));
        out.push_str(&format!("\"kind\":\"{KIND}\","));
        out.push_str(&format!("\"config\":{},", quote(self.config.dir)));
        out.push_str(&format!("\"index\":{},", self.index));
        out.push_str(&format!("\"run_id\":{},", quote(&self.run_id)));
        out.push_str(&format!("\"measured_at\":{},", quote(&self.measured_at)));
        out.push_str(&format!("\"reason\":{},", quote(&self.reason)));
        match &self.supersedes {
            Some(run_id) => out.push_str(&format!("\"supersedes\":{},", quote(run_id))),
            None => out.push_str("\"supersedes\":null,"),
        }
        out.push_str(&format!(
            "\"dataset\":{{\"fingerprint\":{},\"generator_version\":{}}},",
            quote(&self.dataset.fingerprint),
            self.dataset.generator_version
        ));
        let b = &self.behaviour;
        out.push_str(&format!(
            "\"behaviour\":{{\"goal\":{},\"pieces\":{},\"cap\":{},\"width\":{},\"width_source\":{},\
             \"evaluator\":{},\"table\":{},\"table_fnv\":{},\"toggles\":{},\"tail\":{},\"tail_pieces\":{}}},",
            b.goal,
            b.pieces,
            b.cap,
            b.width,
            quote(&b.width_source),
            quote(&b.evaluator),
            quote(&b.table),
            quote(&b.table_fnv),
            quote(&b.toggles),
            b.tail,
            b.tail_pieces
        ));
        out.push_str(&format!(
            "\"split\":{{\"first\":{},\"last\":{},\"n\":{}}},",
            self.split.first,
            self.split.last,
            self.split.n()
        ));
        let s = &self.stats;
        out.push_str(&format!(
            "\"stats\":{{\"n\":{},\"wins\":{},\"n_fail\":{},\"win_rate\":{},\"wilson95\":[{},{}],\
             \"fail_lines_mean\":{},\"fail_lines_median\":{},\
             \"evals_p50\":{},\"evals_p90\":{},\"evals_mean\":{},\"evals_max\":{},\
             \"ms_p50\":{},\"ms_p90\":{},\"ms_max\":{},\
             \"batch_ms\":{},\"cal_ms\":{},\"cal_ms_before\":{},\"cal_ms_after\":{}}},",
            s.n,
            s.wins,
            s.n_fail,
            plain(s.win_rate),
            plain(s.wilson95[0]),
            plain(s.wilson95[1]),
            optional(s.fail_lines_mean.map(plain)),
            optional(s.fail_lines_median.map(|lines| lines.to_string())),
            s.evals_p50,
            s.evals_p90,
            plain(s.evals_mean),
            s.evals_max,
            millis(s.ms_p50),
            millis(s.ms_p90),
            millis(s.ms_max),
            millis(s.batch_ms),
            millis(s.cal_ms),
            millis(s.cal_ms_before),
            millis(s.cal_ms_after)
        ));
        let f = &self.fingerprints;
        out.push_str(&format!(
            "\"fingerprints\":{{\"evals_fnv1a64\":{},\"lines_fnv1a64\":{},\"evals_method\":{},\
             \"lines_method\":{},\"p90_method\":{}}},",
            quote(&f.evals_fnv1a64),
            quote(&f.lines_fnv1a64),
            quote(&f.evals_method),
            quote(&f.lines_method),
            quote(&f.p90_method)
        ));
        let e = &self.engine;
        out.push_str(&format!(
            "\"engine\":{{\"commit\":{},\"short\":{},\"dirty\":{},\"wasm_sha256\":{},\
             \"wasm_bytes\":{},\"profile\":{},\"toolchain\":{}}},",
            quote(&e.commit),
            quote(&e.short),
            e.dirty,
            quote(&e.wasm_sha256),
            e.wasm_bytes,
            quote(&e.profile),
            quote(&e.toolchain)
        ));
        let m = &self.machine;
        out.push_str(&format!(
            "\"machine\":{{\"cpu\":{},\"os\":{},\"runtime\":{},\"substrate\":{},\
             \"browser_or_node\":{},\"rustc\":{},\"node\":{}}},",
            quote(&m.cpu),
            quote(&m.os),
            quote(&m.runtime),
            quote(&m.substrate),
            quote(&m.browser_or_node),
            quote(&m.rustc),
            quote(&m.node)
        ));
        // The flat projection of the label block: one string per fact, for a reader
        // that does not walk nested objects (web/scripts/build-facts.mjs reads
        // `labels.machine`, `labels.runtime`, `labels.date` and `labels.engine_commit`,
        // and refuses to quote a label it cannot name). Derived, never a second
        // opinion: every value here is a field of this row, copied.
        out.push_str(&format!(
            "\"labels\":{{\"machine\":{},\"runtime\":{},\"date\":{},\"engine_commit\":{}}}",
            quote(&format!("{} · {}", m.cpu, m.os)),
            quote(&m.runtime),
            quote(&self.measured_at),
            quote(&self.engine.commit)
        ));
        out.push('}');
        out
    }
}

/// The row ids: `commit/wasm8/E1/w<width>/<date>`. Short, unique per behaviour
/// identity, and stable enough to point `supersedes` at.
pub fn run_id(engine: &Engine, width: u32, measured_at: &str) -> String {
    let wasm = &engine.wasm_sha256;
    let short = if wasm.len() >= 8 { &wasm[..8] } else { wasm.as_str() };
    format!("{}/{short}/{EVALUATOR_ID}/w{width}/{measured_at}", engine.short)
}

/// One row per measurement, in `measurements` order, chained by `index` and
/// `supersedes` so nothing has to be rewritten to record a second run.
pub fn rows(measurements: &[Measurement], labels: &RunLabels) -> Vec<Row> {
    let mut out: Vec<Row> = Vec::with_capacity(measurements.len());
    for (offset, measurement) in measurements.iter().enumerate() {
        let index = labels.first_index + offset as u32;
        let supersedes = match out.last() {
            Some(previous) => Some(previous.run_id.clone()),
            None => labels.first_supersedes.clone(),
        };
        let id = run_id(&labels.engine, measurement.width, &labels.measured_at);
        out.push(Row {
            config: measurement.config,
            index,
            run_id: id,
            measured_at: labels.measured_at.clone(),
            reason: labels.reason.clone(),
            supersedes,
            dataset: labels
                .dataset(&measurement.config)
                .cloned()
                .unwrap_or(DatasetId {
                    fingerprint: String::new(),
                    generator_version: 0,
                }),
            behaviour: Behaviour {
                goal: measurement.config.goal,
                pieces: measurement.config.pieces,
                cap: measurement.config.cap,
                width: measurement.width,
                width_source: "artifact".to_string(),
                evaluator: EVALUATOR_ID.to_string(),
                table: EVALUATOR_TABLE.to_string(),
                table_fnv: table_fnv(),
                toggles: TOGGLES.to_string(),
                tail: true,
                tail_pieces: engine::search::TAIL,
            },
            split: measurement.split,
            stats: measurement.stats(),
            fingerprints: measurement.fingerprints(),
            engine: labels.engine.clone(),
            machine: labels.machine.clone(),
        });
    }
    out
}

// ------------------------------------------------------------ the append-only log

/// The next free `(index, supersedes)` for a configuration's log, without creating
/// it: an absent or empty file starts at `(0, None)`.
pub fn next_index(path: &Path) -> Result<(u32, Option<String>), String> {
    let rows = read(path)?;
    Ok(match rows.last() {
        Some(last) => (last.index + 1, Some(last.run_id.clone())),
        None => (0, None),
    })
}

/// Append one row. The only writer of `reference.jsonl`, and it only appends:
/// `OpenOptions::append(true)`, a check that the index continues the chain, and a new
/// line that cannot be merged into the previous one.
pub fn append(path: &Path, row: &Row) -> Result<(), String> {
    let (next, previous) = next_index(path)?;
    if row.index != next {
        return Err(format!(
            "{}: refusing to append row index {} — the next index is {next}. reference.jsonl is \
             append-only: a re-measurement adds a row, it never replaces one (§9.6)",
            path.display(),
            row.index
        ));
    }
    if row.supersedes != previous {
        return Err(format!(
            "{}: row {} must supersede {:?}, not {:?} (the chain is the file's own history)",
            path.display(),
            row.index,
            previous,
            row.supersedes
        ));
    }
    if let Some(parent) = path.parent() {
        if !parent.is_dir() {
            return Err(format!("{} is not a directory", parent.display()));
        }
    }
    // A file whose last line has no newline would silently glue two rows together.
    if let Ok(bytes) = fs::read(path) {
        if let Some(last) = bytes.last() {
            if *last != b'\n' {
                return Err(format!(
                    "{}: the last line has no newline; refusing to append a row onto it",
                    path.display()
                ));
            }
        }
    }
    let mut file = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .map_err(|error| format!("open {} for append: {error}", path.display()))?;
    file.write_all(format!("{}\n", row.render()).as_bytes())
        .map_err(|error| format!("append to {}: {error}", path.display()))
}

/// Every row of a log, in file order. An absent or empty file is no rows, not an
/// error: the log is committed empty until the first Reference run lands.
pub fn read(path: &Path) -> Result<Vec<Row>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let mut rows = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let row = parse_row(line, path, number + 1)?;
        if let Some(previous) = rows.last() {
            let previous: &Row = previous;
            if row.index != previous.index + 1 {
                return Err(format!(
                    "{}:{}: row index {} follows {} — the log is a chain, not a set",
                    path.display(),
                    number + 1,
                    row.index,
                    previous.index
                ));
            }
            if row.measured_at < previous.measured_at {
                return Err(format!(
                    "{}:{}: measured_at {} is before the previous row's {} — `tail -1` must be the \
                     latest row",
                    path.display(),
                    number + 1,
                    row.measured_at,
                    previous.measured_at
                ));
            }
        }
        rows.push(row);
    }
    Ok(rows)
}

fn parse_row(line: &str, path: &Path, number: usize) -> Result<Row, String> {
    let where_ = format!("{}:{number}", path.display());
    let value = json::parse(line).map_err(|error| format!("{where_}: {error}"))?;

    let format = value
        .get("format")
        .and_then(Value::as_u32)
        .ok_or_else(|| format!("{where_}: format is missing"))?;
    if format != FORMAT {
        return Err(format!(
            "{where_}: format {format} is not this reader's {FORMAT}"
        ));
    }
    let kind = text_of(&value, "kind").map_err(|error| format!("{where_}: {error}"))?;
    if kind != KIND {
        return Err(format!("{where_}: kind {kind:?} is not {KIND:?}"));
    }
    let dir = text_of(&value, "config").map_err(|error| format!("{where_}: {error}"))?;
    let config = Config::from_name(&dir)
        .ok_or_else(|| format!("{where_}: unknown configuration {dir:?}"))?;

    let dataset = value.get("dataset").ok_or_else(|| format!("{where_}: dataset is missing"))?;
    let behaviour = value.get("behaviour").ok_or_else(|| format!("{where_}: behaviour is missing"))?;
    let split = value.get("split").ok_or_else(|| format!("{where_}: split is missing"))?;
    let stats = value.get("stats").ok_or_else(|| format!("{where_}: stats is missing"))?;
    let fingerprints = value
        .get("fingerprints")
        .ok_or_else(|| format!("{where_}: fingerprints is missing"))?;
    let engine = value.get("engine").ok_or_else(|| format!("{where_}: engine is missing"))?;
    let machine = value.get("machine").ok_or_else(|| format!("{where_}: machine is missing"))?;
    Ok(Row {
        config,
        index: uint_of(&value, "index").map_err(|error| format!("{where_}: {error}"))?,
        run_id: text_of(&value, "run_id").map_err(|error| format!("{where_}: {error}"))?,
        measured_at: text_of(&value, "measured_at").map_err(|error| format!("{where_}: {error}"))?,
        reason: text_of(&value, "reason").map_err(|error| format!("{where_}: {error}"))?,
        supersedes: match value.get("supersedes") {
            Some(Value::Null) | None => None,
            Some(Value::String(text)) => Some(text.clone()),
            Some(other) => {
                return Err(format!(
                    "{where_}: supersedes is {}",
                    other.kind()
                ))
            }
        },
        dataset: DatasetId {
            fingerprint: text_of(dataset, "fingerprint").map_err(|error| format!("{where_}: {error}"))?,
            generator_version: uint_of(dataset, "generator_version")
                .map_err(|error| format!("{where_}: {error}"))?,
        },
        behaviour: Behaviour {
            goal: uint_of(behaviour, "goal").map_err(|error| format!("{where_}: {error}"))?,
            pieces: uint_of(behaviour, "pieces").map_err(|error| format!("{where_}: {error}"))?,
            cap: uint_of(behaviour, "cap").map_err(|error| format!("{where_}: {error}"))?,
            width: uint_of(behaviour, "width").map_err(|error| format!("{where_}: {error}"))?,
            width_source: text_of(behaviour, "width_source")
                .map_err(|error| format!("{where_}: {error}"))?,
            evaluator: text_of(behaviour, "evaluator").map_err(|error| format!("{where_}: {error}"))?,
            table: text_of(behaviour, "table").map_err(|error| format!("{where_}: {error}"))?,
            table_fnv: text_of(behaviour, "table_fnv").map_err(|error| format!("{where_}: {error}"))?,
            toggles: text_of(behaviour, "toggles").map_err(|error| format!("{where_}: {error}"))?,
            tail: behaviour
                .get("tail")
                .and_then(Value::as_bool)
                .ok_or_else(|| format!("{where_}: behaviour.tail is missing"))?,
            tail_pieces: uint_of(behaviour, "tail_pieces").map_err(|error| format!("{where_}: {error}"))?
                as usize,
        },
        split: Split {
            first: uint_of(split, "first").map_err(|error| format!("{where_}: {error}"))?,
            last: uint_of(split, "last").map_err(|error| format!("{where_}: {error}"))?,
        },
        stats: stats_of(stats).map_err(|error| format!("{where_}: {error}"))?,
        fingerprints: Fingerprints {
            evals_fnv1a64: text_of(fingerprints, "evals_fnv1a64")
                .map_err(|error| format!("{where_}: {error}"))?,
            lines_fnv1a64: text_of(fingerprints, "lines_fnv1a64")
                .map_err(|error| format!("{where_}: {error}"))?,
            evals_method: text_of(fingerprints, "evals_method")
                .map_err(|error| format!("{where_}: {error}"))?,
            lines_method: text_of(fingerprints, "lines_method")
                .map_err(|error| format!("{where_}: {error}"))?,
            p90_method: text_of(fingerprints, "p90_method")
                .map_err(|error| format!("{where_}: {error}"))?,
        },
        engine: Engine {
            commit: text_of(engine, "commit").map_err(|error| format!("{where_}: {error}"))?,
            short: text_of(engine, "short").map_err(|error| format!("{where_}: {error}"))?,
            dirty: engine
                .get("dirty")
                .and_then(Value::as_bool)
                .ok_or_else(|| format!("{where_}: engine.dirty is missing"))?,
            wasm_sha256: text_of(engine, "wasm_sha256").map_err(|error| format!("{where_}: {error}"))?,
            wasm_bytes: engine
                .get("wasm_bytes")
                .and_then(Value::as_u32)
                .map(|bytes| bytes as u64)
                .ok_or_else(|| format!("{where_}: engine.wasm_bytes is missing"))?,
            profile: text_of(engine, "profile").map_err(|error| format!("{where_}: {error}"))?,
            toolchain: text_of(engine, "toolchain").map_err(|error| format!("{where_}: {error}"))?,
        },
        machine: machine_of(machine).map_err(|error| format!("{where_}: {error}"))?,
    })
}

/// A row's or a golden's `stats` object. One reader, so the two files cannot drift.
pub(crate) fn stats_of(value: &Value) -> Result<Stats, String> {
    let wilson = value
        .get("wilson95")
        .and_then(Value::as_array)
        .filter(|array| array.len() == 2)
        .ok_or("wilson95 must be two numbers")?;
    Ok(Stats {
        n: uint_of(value, "n")?,
        wins: uint_of(value, "wins")?,
        n_fail: uint_of(value, "n_fail")?,
        win_rate: number_of(value, "win_rate")?,
        wilson95: [
            wilson[0].as_f64().ok_or("wilson95[0] is not a number")?,
            wilson[1].as_f64().ok_or("wilson95[1] is not a number")?,
        ],
        fail_lines_mean: optional_number(value, "fail_lines_mean")?,
        fail_lines_median: optional_uint(value, "fail_lines_median")?,
        evals_p50: uint_of(value, "evals_p50")?,
        evals_p90: uint_of(value, "evals_p90")?,
        evals_mean: number_of(value, "evals_mean")?,
        evals_max: uint_of(value, "evals_max")?,
        ms_p50: number_of(value, "ms_p50")?,
        ms_p90: number_of(value, "ms_p90")?,
        ms_max: number_of(value, "ms_max")?,
        batch_ms: number_of(value, "batch_ms")?,
        cal_ms: number_of(value, "cal_ms")?,
        cal_ms_before: number_of(value, "cal_ms_before")?,
        cal_ms_after: number_of(value, "cal_ms_after")?,
    })
}

/// A row's or a golden's `machine` object.
pub(crate) fn machine_of(value: &Value) -> Result<Machine, String> {
    Ok(Machine {
        cpu: text_of(value, "cpu")?,
        os: text_of(value, "os")?,
        runtime: text_of(value, "runtime")?,
        substrate: text_of(value, "substrate")?,
        browser_or_node: text_of(value, "browser_or_node")?,
        rustc: text_of(value, "rustc")?,
        node: text_of(value, "node")?,
    })
}

pub(crate) fn text_of(value: &Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(|text| text.to_string())
        .ok_or_else(|| format!("{key} is missing or not a string"))
}

pub(crate) fn number_of(value: &Value, key: &str) -> Result<f64, String> {
    value
        .get(key)
        .and_then(Value::as_f64)
        .ok_or_else(|| format!("{key} is missing or not a number"))
}

pub(crate) fn uint_of(value: &Value, key: &str) -> Result<u32, String> {
    value
        .get(key)
        .and_then(Value::as_u32)
        .ok_or_else(|| format!("{key} is missing or not a non-negative integer"))
}

pub(crate) fn optional_number(value: &Value, key: &str) -> Result<Option<f64>, String> {
    match value.get(key) {
        Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_f64()
            .map(Some)
            .ok_or_else(|| format!("{key} is neither null nor a number")),
        None => Err(format!("{key} is missing (a missing row is not a null one)")),
    }
}

pub(crate) fn optional_uint(value: &Value, key: &str) -> Result<Option<u32>, String> {
    match value.get(key) {
        Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u32()
            .map(Some)
            .ok_or_else(|| format!("{key} is neither null nor a non-negative integer")),
        None => Err(format!("{key} is missing (a missing row is not a null one)")),
    }
}

// ---------------------------------------------------------------- the page

/// The fields the page reads, figure by figure (notes/14 §2.2's R1–R9, §2.4's
/// "beside the Reference run's same rows"). The paths are JSON paths into one row, so
/// the page does not re-derive a figure from a different row: every number on screen
/// is a field in the row it names.
///
/// It is a *contract*, and `bench/tests/reference.rs` walks it against a rendered row:
/// a renamed field breaks the test before it breaks the page.
pub const PAGE_FIELDS: [(&str, &[&str]); 9] = [
    // R1: the CI travels with the rate — a bare 100 % on a saturated bed misleads.
    ("R1 win rate", &["stats.win_rate", "stats.wilson95"]),
    // R2: the numerator and the denominator the split names, never a page constant.
    ("R2 games won", &["stats.wins", "split.n"]),
    // R3/R4: `null` when there were no failures, beside the count that says so.
    (
        "R3 mean Lines on failure",
        &["stats.fail_lines_mean", "stats.n_fail"],
    ),
    (
        "R4 median Lines on failure",
        &["stats.fail_lines_median", "stats.n_fail"],
    ),
    // R5/R6: the two per-solve rows, and the method that produced the percentile.
    (
        "R5 Evaluations per Solve",
        &["stats.evals_p50", "stats.evals_p90", "fingerprints.p90_method"],
    ),
    (
        "R6 ms per Solve",
        &["stats.ms_p50", "stats.ms_p90", "machine.substrate"],
    ),
    // R7: the whole split's wall clock, per configuration; the page's total is the
    // sum of two committed rows, which is a display of data and not a measurement.
    ("R7 total ms", &["stats.batch_ms"]),
    // R8: the label block, without which R1–R7 are numbers with no machine.
    (
        "R8 Reference run labels",
        &[
            "engine.commit",
            "engine.wasm_sha256",
            "engine.profile",
            "machine.cpu",
            "machine.os",
            "machine.runtime",
            "machine.substrate",
            "machine.browser_or_node",
            "measured_at",
        ],
    ),
    // R9: what the row was measured on, so a stale row can be shown with a banner
    // rather than hidden.
    (
        "R9 games played",
        &["split.first", "split.last", "dataset.fingerprint"],
    ),
];

/// Walk a JSON path like `stats.evals_p90` into a parsed row.
pub fn resolve<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(value, |current, key| current.get(key))
}

// --------------------------------------------------------------- formatting

/// A float as short as it round-trips: `1.0` prints as `1`, which is a JSON number.
pub(crate) fn plain(value: f64) -> String {
    if value.is_finite() {
        format!("{value}")
    } else {
        "null".to_string()
    }
}

/// Milliseconds to three decimals: sub-microsecond precision is noise, and the row
/// is a reading rather than a benchmark.
pub(crate) fn millis(value: f64) -> String {
    if value.is_finite() {
        format!("{value:.3}")
    } else {
        "null".to_string()
    }
}

fn optional(value: Option<String>) -> String {
    value.unwrap_or_else(|| "null".to_string())
}

/// A JSON string: the row's own text fields (a reason, a CPU name), escaped so a
/// stray quote cannot make the line unparseable.
pub fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_build_flags_label_is_the_committed_profile() {
        // §9.6 wants "build flags" in the label, and a hand-typed string age badly.
        // The committed `[profile.release]` block is the source; this asserts the
        // label still says what it says.
        let text = fs::read_to_string(root().join("Cargo.toml")).expect("Cargo.toml");
        let release = text
            .split("[profile.release]")
            .nth(1)
            .expect("a [profile.release] block");
        for flag in ["lto = \"fat\"", "codegen-units = 1", "panic = \"abort\"", "strip = true"] {
            assert!(
                release.contains(flag),
                "Cargo.toml no longer sets {flag}; the row's profile label says: {ENGINE_PROFILE}"
            );
        }
    }

    #[test]
    fn the_table_fingerprint_is_the_shipped_weight_table() {
        // Ticket 08's ladder names `E1_PUBLISHED` as what ships, and this is the same
        // eight integers hashed the same way as ticket 07's record.
        assert_eq!(table_fnv(), crate::tuning::table_fnv(&engine::E1_PUBLISHED.as_array()));
        assert!(table_fnv().starts_with("0x"), "{}", table_fnv());
    }

    #[test]
    fn a_median_of_nothing_is_null_and_of_two_is_the_lower() {
        assert_eq!(median_u32(&[]), None);
        assert_eq!(median_u32(&[3]), Some(3));
        assert_eq!(median_u32(&[3, 5]), Some(3));
        assert_eq!(median_u32(&[3, 4, 5]), Some(4));
    }

    #[test]
    fn the_split_knows_its_own_size() {
        assert_eq!(Split::TEST.n(), 1000);
        assert_eq!(Split::TEST.label(), "0-999");
        assert_eq!(Split::TUNING.n(), 1000);
        assert!(Split::from_text("tuning").is_ok());
        assert!(Split::from_text("2000-2999").is_err());
    }
}
