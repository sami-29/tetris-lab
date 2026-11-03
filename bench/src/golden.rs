//! §9.2 gate 6's second half: the exact evaluations-to-solve fingerprint against the
//! committed golden values, and §9.3's "4x wall-clock sanity bound".
//!
//! The shape of the argument matters more than the code:
//!
//! * **Behaviour is exact.** `evals` and `lines` are integers produced by a
//!   deterministic search over a fixed Dataset: no clock, no RNG, no substrate takes
//!   part (§9.6). A difference therefore *cannot* be a re-measurement — it is a
//!   behaviour change, which is a version bump. There is no tolerance, no retry and
//!   no "within 0.1 %".
//! * **The identity is compared first.** A golden is only meaningful against the
//!   games and the artifact it was pinned on, so the gate compares the Dataset
//!   fingerprint, the engine hash, the width (a golden number, §9.5) and the
//!   evaluator/toggles labels *before* it compares a single seed. When they differ it
//!   says which of them changed and refuses to call that a drift: the golden has to be
//!   re-pinned deliberately.
//! * **The clock is loose, and only the clock.** `wall_clock_factor` is 4, and the
//!   comparison is normalised by an in-run calibration slice, because a shared box's
//!   `batch_ms` varied 2.06x across five identical reps (notes/09 §0.3). A slow box
//!   can only ever trip this one line.
//!
//! `bench golden update --reason "…"` re-pins the file, and it refuses to do so
//! unless a committed Reference row already carries the same vectors: the golden is
//! pinned *from* a recorded measurement, never from an unrecorded one (§9.6).

use std::fs;
use std::path::{Path, PathBuf};

use crate::dataset::{self, Config};
use crate::fingerprint::{self, Fnv};
use crate::gate::{Check, State};
use crate::json::{self, Value};
use crate::reference::{
    self, DatasetId, Engine, Machine, Measurement, RunLabels, Split, Stats,
};

/// The golden file's format. A reader that does not know it must refuse the file.
pub const FORMAT: u32 = 1;
/// The golden file's `kind`.
pub const KIND: &str = "golden-evals";

/// §9.3's loose bound. Behaviour is compared exactly; time gets a factor and a
/// normaliser, because wall-clock assertions on shared runners are flaky (ADR 0002).
pub const WALL_CLOCK_FACTOR: f64 = 4.0;

/// How many mismatches a failure message names before it stops counting.
pub const MAX_NAMED: usize = 6;

/// The identity a golden is keyed on: what has to be equal *before* the seeds are
/// compared. Deliberately no clock and no commit: a doc-only commit or a faster box
/// must not fail gate 6b (notes/09 §2.3, Q2).
#[derive(Clone, Debug, PartialEq)]
pub struct Identity {
    pub engine: Engine,
    pub machine: Machine,
    pub datasets: Vec<(String, DatasetId)>,
    pub behaviour: Behaviour,
}

/// The behaviour half of the identity — the numbers and the switches, never the bed
/// (each configuration carries its own goal in [`GoldenConfig`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Behaviour {
    pub width: u32,
    pub width_source: String,
    pub evaluator: String,
    pub table: String,
    pub table_fnv: String,
    pub toggles: String,
    pub tail: bool,
    pub tail_pieces: usize,
}

/// One configuration's golden: the two per-seed vectors, their digests, and the
/// quality rows a reader can eyeball against the Reference row.
#[derive(Clone, Debug, PartialEq)]
pub struct GoldenConfig {
    pub config: Config,
    pub stats: Stats,
    pub evals: Vec<u32>,
    pub lines: Vec<u32>,
    pub evals_fnv1a64: String,
    pub lines_fnv1a64: String,
}

/// `data/measurements/golden-evals.json`, parsed.
#[derive(Clone, Debug, PartialEq)]
pub struct Golden {
    pub format: u32,
    pub kind: String,
    pub generated_at: String,
    pub reason: String,
    pub supersedes: Option<String>,
    pub engine: Engine,
    pub machine: Machine,
    pub datasets: Vec<(String, DatasetId)>,
    pub behaviour: Behaviour,
    pub split: Split,
    pub configs: Vec<GoldenConfig>,
    /// `config` → the `run_id` of the Reference row this golden was pinned from.
    pub reference: Vec<(String, String)>,
}

impl Golden {
    pub fn dataset(&self, dir: &str) -> Option<&DatasetId> {
        self.datasets
            .iter()
            .find(|(name, _)| name == dir)
            .map(|(_, id)| id)
    }

    pub fn entry(&self, dir: &str) -> Option<&GoldenConfig> {
        self.configs.iter().find(|entry| entry.config.dir == dir)
    }

    pub fn entry_mut(&mut self, dir: &str) -> Option<&mut GoldenConfig> {
        self.configs.iter_mut().find(|entry| entry.config.dir == dir)
    }

    pub fn reference_run(&self, dir: &str) -> Option<&str> {
        self.reference
            .iter()
            .find(|(name, _)| name == dir)
            .map(|(_, run_id)| run_id.as_str())
    }

    /// The two per-seed vectors of one configuration, or `None`.
    pub fn vectors(&self, dir: &str) -> Option<(&[u32], &[u32])> {
        self.entry(dir)
            .map(|entry| (entry.evals.as_slice(), entry.lines.as_slice()))
    }
    /// The identity this golden is keyed on.
    pub fn identity(&self) -> Identity {
        Identity {
            engine: self.engine.clone(),
            machine: self.machine.clone(),
            datasets: self.datasets.clone(),
            behaviour: self.behaviour.clone(),
        }
    }

    /// Recompute the two digests from the vectors. `render` calls this, and `load`
    /// verifies it: a hand-edited vector with a stale digest is a corrupt file, not a
    /// behaviour change, and the two must not be reported as the same thing.
    pub fn refresh(&mut self) {
        for entry in &mut self.configs {
            entry.evals_fnv1a64 = digest_evals(&entry.evals);
            entry.lines_fnv1a64 = digest_lines(&entry.lines);
        }
    }
}

/// §1.2's evaluations digest: FNV-1a-64 over the vector, 4 bytes little-endian each.
pub fn digest_evals(evals: &[u32]) -> String {
    let mut hash = Fnv::new();
    for value in evals {
        hash.u32_le(*value);
    }
    fingerprint::hex(hash.finish())
}

/// §1.2's lines digest: FNV-1a-64 over the vector, one byte each.
pub fn digest_lines(lines: &[u32]) -> String {
    let mut hash = Fnv::new();
    for value in lines {
        hash.byte(*value as u8);
    }
    fingerprint::hex(hash.finish())
}

/// The identity a measurement was taken under: the engine label, the box, the
/// Dataset identities and the behaviour labels. One constructor, so `bench golden
/// update`, `bench golden check` and gate 6b cannot disagree about what is being
/// compared.
pub fn identity(
    engine_label: Engine,
    machine_label: Machine,
    datasets: Vec<(String, DatasetId)>,
    width: u32,
) -> Identity {
    Identity {
        engine: engine_label,
        machine: machine_label,
        datasets,
        behaviour: Behaviour {
            width,
            width_source: "artifact".to_string(),
            evaluator: reference::EVALUATOR_ID.to_string(),
            table: reference::EVALUATOR_TABLE.to_string(),
            table_fnv: reference::table_fnv(),
            toggles: reference::TOGGLES.to_string(),
            tail: true,
            tail_pieces: engine::search::TAIL,
        },
    }
}

// ------------------------------------------------------------------- reading

pub fn load(path: &Path) -> Result<Golden, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    from_text(&text).map_err(|error| format!("{}: {error}", path.display()))
}

pub fn from_text(text: &str) -> Result<Golden, String> {
    let value = json::parse(text)?;
    let format = value
        .get("format")
        .and_then(Value::as_u32)
        .ok_or("format is missing or not a positive integer")?;
    if format != FORMAT {
        return Err(format!("format {format} is not this reader's {FORMAT}"));
    }
    let kind = reference::text_of(&value, "kind")?;
    if kind != KIND {
        return Err(format!("kind {kind:?} is not {KIND:?}"));
    }

    let engine = value.get("engine").ok_or("engine is missing")?;
    let machine = value.get("machine").ok_or("machine is missing")?;
    let behaviour = value.get("behaviour").ok_or("behaviour is missing")?;
    let split = value.get("split").ok_or("split is missing")?;
    let datasets = value.get("datasets").ok_or("datasets is missing")?;
    let configs = value.get("configs").ok_or("configs is missing")?;

    let mut dataset_ids = Vec::new();
    for config in dataset::CONFIGS {
        let id = datasets
            .get(config.dir)
            .ok_or_else(|| format!("datasets.{} is missing", config.dir))?;
        dataset_ids.push((
            config.dir.to_string(),
            DatasetId {
                fingerprint: reference::text_of(id, "fingerprint")?,
                generator_version: reference::uint_of(id, "generator_version")?,
            },
        ));
    }

    let mut entries = Vec::new();
    for config in dataset::CONFIGS {
        let entry = configs
            .get(config.dir)
            .ok_or_else(|| format!("configs.{} is missing", config.dir))?;
        let stats = entry.get("stats").ok_or_else(|| {
            format!("configs.{}.stats is missing", config.dir)
        })?;
        let evals = vector(entry, "evals", config.dir)?;
        let lines = vector(entry, "lines", config.dir)?;
        let recorded_evals = reference::text_of(entry, "evals_fnv1a64")?;
        let recorded_lines = reference::text_of(entry, "lines_fnv1a64")?;
        let computed_evals = digest_evals(&evals);
        let computed_lines = digest_lines(&lines);
        if recorded_evals != computed_evals || recorded_lines != computed_lines {
            return Err(format!(
                "configs.{}: the stored digests are not the vectors' ({recorded_evals}/{recorded_lines} \
                 recorded, {computed_evals}/{computed_lines} computed) — the file was edited by hand, \
                 which is not the same thing as a measured change",
                config.dir
            ));
        }
        entries.push(GoldenConfig {
            config,
            stats: reference::stats_of(stats)?,
            evals,
            lines,
            evals_fnv1a64: recorded_evals,
            lines_fnv1a64: recorded_lines,
        });
    }

    let mut reference_ids = Vec::new();
    if let Some(reference) = value.get("reference") {
        for config in dataset::CONFIGS {
            if let Some(run_id) = reference.get(config.dir).and_then(Value::as_str) {
                reference_ids.push((config.dir.to_string(), run_id.to_string()));
            }
        }
    }

    Ok(Golden {
        format,
        kind,
        generated_at: reference::text_of(&value, "generated_at")?,
        reason: reference::text_of(&value, "reason")?,
        supersedes: match value.get("supersedes") {
            Some(Value::String(text)) => Some(text.clone()),
            _ => None,
        },
        engine: Engine {
            commit: reference::text_of(engine, "commit")?,
            short: reference::text_of(engine, "short")?,
            dirty: engine.get("dirty").and_then(Value::as_bool).unwrap_or(false),
            wasm_sha256: reference::text_of(engine, "wasm_sha256")?,
            wasm_bytes: reference::uint_of(engine, "wasm_bytes")? as u64,
            profile: reference::text_of(engine, "profile")?,
            toolchain: reference::text_of(engine, "toolchain")?,
        },
        machine: reference::machine_of(machine)?,
        datasets: dataset_ids,
        behaviour: Behaviour {
            width: reference::uint_of(behaviour, "width")?,
            width_source: reference::text_of(behaviour, "width_source")?,
            evaluator: reference::text_of(behaviour, "evaluator")?,
            table: reference::text_of(behaviour, "table")?,
            table_fnv: reference::text_of(behaviour, "table_fnv")?,
            toggles: reference::text_of(behaviour, "toggles")?,
            tail: behaviour.get("tail").and_then(Value::as_bool).unwrap_or(true),
            tail_pieces: reference::uint_of(behaviour, "tail_pieces")? as usize,
        },
        split: Split {
            first: reference::uint_of(split, "first")?,
            last: reference::uint_of(split, "last")?,
        },
        configs: entries,
        reference: reference_ids,
    })
}

fn vector(entry: &Value, key: &str, config: &str) -> Result<Vec<u32>, String> {
    let array = entry
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("configs.{config}.{key} is missing or not an array"))?;
    array
        .iter()
        .enumerate()
        .map(|(index, value)| {
            value
                .as_u32()
                .ok_or_else(|| format!("configs.{config}.{key}[{index}] is not an integer"))
        })
        .collect()
}

// ------------------------------------------------------------------- writing

/// The file, pretty-printed two spaces a level, with one number per line in the
/// per-seed vectors: the line offset inside `evals` **is** the seed, which is what
/// makes "which seed moved" a question the diff answers on its own.
pub fn render(golden: &Golden) -> String {
    let mut out = String::with_capacity(64 * 1024);
    out.push_str("{\n");
    field(&mut out, 1, "format", &golden.format.to_string(), true);
    field(&mut out, 1, "kind", &reference::quote(&golden.kind), true);
    field(&mut out, 1, "generated_at", &reference::quote(&golden.generated_at), true);
    field(&mut out, 1, "reason", &reference::quote(&golden.reason), true);
    match &golden.supersedes {
        Some(text) => field(&mut out, 1, "supersedes", &reference::quote(text), true),
        None => field(&mut out, 1, "supersedes", "null", true),
    }

    out.push_str("  \"compare\": {\n");
    field(&mut out, 2, "rule", "\"exact\"", true);
    out.push_str(
        "    \"quality_rows\": [\"win rate\", \"mean and median Lines on failure\", \
         \"Evaluations per Solve\", \"Lines to solve\"],\n",
    );
    field(&mut out, 2, "tolerance", "\"none\"", true);
    field(
        &mut out,
        2,
        "why",
        &reference::quote(
            "an Evaluation count and a Lines count are integers from a deterministic search over a \
             fixed Dataset: they cannot vary with clock speed, load, thread count or substrate, so a \
             difference is a behaviour change (spec 9.6) — a version bump, not a re-measurement",
        ),
        true,
    );
    out.push_str("    \"fingerprints\": {\n");
    field(
        &mut out,
        3,
        "evals",
        &reference::quote("fnv1a64 over the evaluations, 4 bytes little-endian each, seed order"),
        true,
    );
    field(
        &mut out,
        3,
        "lines",
        &reference::quote("fnv1a64 over the lines, 1 byte each, seed order"),
        false,
    );
    out.push_str("    },\n");
    out.push_str(
        "    \"identity_first\": [\"datasets[*].fingerprint\", \"engine.wasm_sha256\", \
         \"behaviour.width\", \"behaviour.evaluator\", \"behaviour.toggles\"]\n",
    );
    out.push_str("  },\n");

    out.push_str("  \"bound\": {\n");
    field(&mut out, 2, "wall_clock_factor", "4", true);
    field(&mut out, 2, "measured", "\"batch_ms over the whole Split\"", true);
    field(
        &mut out,
        2,
        "normalised_by",
        &reference::quote(
            "cal_ms: the mean of two slices of 10 solves of seed 0 of goal12-p100-cap7 at width 200, \
             timed in the same process as the batch",
        ),
        true,
    );
    field(
        &mut out,
        2,
        "rule",
        &reference::quote(
            "batch_ms/cal_ms <= 4 x (the golden's batch_ms/cal_ms). The raw ratio is reported and \
             never asserted: on a shared box it varied 2.06x across five identical reps (notes/09 \
             0.3), which is inside its own noise",
        ),
        true,
    );
    field(
        &mut out,
        2,
        "why",
        &reference::quote(
            "ADR 0002: CI guards the fingerprint, not the clock. A wall-clock assertion on a shared \
             runner is flaky, so this bound exists to catch a catastrophic regression (the tail \
             exploding, an O(n^2) dedup), never a 20 % slowdown",
        ),
        false,
    );
    out.push_str("  },\n");

    out.push_str("  \"engine\": {\n");
    field(&mut out, 2, "commit", &reference::quote(&golden.engine.commit), true);
    field(&mut out, 2, "short", &reference::quote(&golden.engine.short), true);
    field(&mut out, 2, "dirty", &golden.engine.dirty.to_string(), true);
    field(
        &mut out,
        2,
        "wasm_sha256",
        &reference::quote(&golden.engine.wasm_sha256),
        true,
    );
    field(&mut out, 2, "wasm_bytes", &golden.engine.wasm_bytes.to_string(), true);
    field(&mut out, 2, "profile", &reference::quote(&golden.engine.profile), true);
    field(
        &mut out,
        2,
        "toolchain",
        &reference::quote(&golden.engine.toolchain),
        false,
    );
    out.push_str("  },\n");

    out.push_str("  \"machine\": {\n");
    for (index, (key, value)) in [
        ("cpu", &golden.machine.cpu),
        ("os", &golden.machine.os),
        ("runtime", &golden.machine.runtime),
        ("substrate", &golden.machine.substrate),
        ("browser_or_node", &golden.machine.browser_or_node),
        ("rustc", &golden.machine.rustc),
        ("node", &golden.machine.node),
    ]
    .iter()
    .enumerate()
    {
        field(&mut out, 2, key, &reference::quote(value), index + 1 < 7);
    }
    out.push_str("  },\n");

    out.push_str("  \"datasets\": {\n");
    for (index, (name, id)) in golden.datasets.iter().enumerate() {
        out.push_str(&format!("    {}: {{\n", reference::quote(name)));
        field(&mut out, 3, "fingerprint", &reference::quote(&id.fingerprint), true);
        field(
            &mut out,
            3,
            "generator_version",
            &id.generator_version.to_string(),
            false,
        );
        out.push_str(&format!(
            "    }}{}\n",
            if index + 1 < golden.datasets.len() { "," } else { "" }
        ));
    }
    out.push_str("  },\n");

    out.push_str("  \"behaviour\": {\n");
    field(&mut out, 2, "width", &golden.behaviour.width.to_string(), true);
    field(
        &mut out,
        2,
        "width_source",
        &reference::quote(&golden.behaviour.width_source),
        true,
    );
    field(
        &mut out,
        2,
        "evaluator",
        &reference::quote(&golden.behaviour.evaluator),
        true,
    );
    field(&mut out, 2, "table", &reference::quote(&golden.behaviour.table), true);
    field(
        &mut out,
        2,
        "table_fnv",
        &reference::quote(&golden.behaviour.table_fnv),
        true,
    );
    field(
        &mut out,
        2,
        "toggles",
        &reference::quote(&golden.behaviour.toggles),
        true,
    );
    field(&mut out, 2, "tail", &golden.behaviour.tail.to_string(), true);
    field(
        &mut out,
        2,
        "tail_pieces",
        &golden.behaviour.tail_pieces.to_string(),
        false,
    );
    out.push_str("  },\n");

    out.push_str("  \"split\": {\n");
    field(&mut out, 2, "first", &golden.split.first.to_string(), true);
    field(&mut out, 2, "last", &golden.split.last.to_string(), true);
    field(&mut out, 2, "n", &golden.split.n().to_string(), false);
    out.push_str("  },\n");

    out.push_str("  \"configs\": {\n");
    for (index, entry) in golden.configs.iter().enumerate() {
        out.push_str(&format!("    {}: {{\n", reference::quote(entry.config.dir)));
        out.push_str("      \"stats\": {\n");
        push_stats(&mut out, &entry.stats);
        out.push_str("      },\n");
        field(
            &mut out,
            3,
            "evals_fnv1a64",
            &reference::quote(&entry.evals_fnv1a64),
            true,
        );
        field(
            &mut out,
            3,
            "lines_fnv1a64",
            &reference::quote(&entry.lines_fnv1a64),
            true,
        );
        push_vector(&mut out, "evals", &entry.evals);
        out.push_str(",\n");
        push_vector(&mut out, "lines", &entry.lines);
        out.push('\n');
        out.push_str(&format!(
            "    }}{}\n",
            if index + 1 < golden.configs.len() { "," } else { "" }
        ));
    }
    out.push_str("  },\n");

    out.push_str("  \"reference\": {\n");
    for (index, (name, run_id)) in golden.reference.iter().enumerate() {
        let comma = index + 1 < golden.reference.len();
        out.push_str(&format!(
            "    {}: {}{}\n",
            reference::quote(name),
            reference::quote(run_id),
            if comma { "," } else { "" }
        ));
    }
    out.push_str("  }\n");
    out.push_str("}\n");
    out
}

fn field(out: &mut String, depth: usize, key: &str, value: &str, comma: bool) {
    let _ = depth;
    out.push_str("  ");
    out.push_str("  ");
    out.push_str(&reference::quote(key));
    out.push_str(": ");
    out.push_str(value);
    if comma {
        out.push(',');
    }
    out.push('\n');
}

fn push_stats(out: &mut String, stats: &Stats) {
    let fields: Vec<(String, String)> = vec![
        ("n".to_string(), stats.n.to_string()),
        ("wins".to_string(), stats.wins.to_string()),
        ("n_fail".to_string(), stats.n_fail.to_string()),
        ("win_rate".to_string(), reference::plain(stats.win_rate)),
        (
            "wilson95".to_string(),
            format!(
                "[{}, {}]",
                reference::plain(stats.wilson95[0]),
                reference::plain(stats.wilson95[1])
            ),
        ),
        (
            "fail_lines_mean".to_string(),
            stats
                .fail_lines_mean
                .map(reference::plain)
                .unwrap_or_else(|| "null".to_string()),
        ),
        (
            "fail_lines_median".to_string(),
            stats
                .fail_lines_median
                .map(|lines| lines.to_string())
                .unwrap_or_else(|| "null".to_string()),
        ),
        ("evals_p50".to_string(), stats.evals_p50.to_string()),
        ("evals_p90".to_string(), stats.evals_p90.to_string()),
        ("evals_mean".to_string(), reference::plain(stats.evals_mean)),
        ("evals_max".to_string(), stats.evals_max.to_string()),
        ("ms_p50".to_string(), reference::millis(stats.ms_p50)),
        ("ms_p90".to_string(), reference::millis(stats.ms_p90)),
        ("ms_max".to_string(), reference::millis(stats.ms_max)),
        ("batch_ms".to_string(), reference::millis(stats.batch_ms)),
        ("cal_ms".to_string(), reference::millis(stats.cal_ms)),
    ];
    for (index, (key, value)) in fields.iter().enumerate() {
        out.push_str("        ");
        out.push_str(&reference::quote(key));
        out.push_str(": ");
        out.push_str(value);
        if index + 1 < fields.len() {
            out.push(',');
        }
        out.push('\n');
    }
}

fn push_vector(out: &mut String, key: &str, values: &[u32]) {
    out.push_str("      ");
    out.push_str(&reference::quote(key));
    out.push_str(": [\n");
    for (index, value) in values.iter().enumerate() {
        out.push_str(&format!(
            "        {value}{}\n",
            if index + 1 < values.len() { "," } else { "" }
        ));
    }
    out.push_str("      ]");
}

/// Build the golden from a measured Reference run. `reference_rows` are the run ids
/// the vectors came from, so the file always names a committed row.
pub fn from_measurements(
    measurements: &[Measurement],
    labels: &RunLabels,
    reference_rows: &[(String, String)],
    generated_at: &str,
    reason: &str,
    supersedes: Option<String>,
) -> Golden {
    let configs = measurements
        .iter()
        .map(|measurement| {
            let evals = measurement.evals();
            let lines = measurement.lines();
            GoldenConfig {
                config: measurement.config,
                stats: measurement.stats(),
                evals_fnv1a64: digest_evals(&evals),
                lines_fnv1a64: digest_lines(&lines),
                evals,
                lines,
            }
        })
        .collect();
    let width = measurements.first().map(|measurement| measurement.width).unwrap_or(0);
    Golden {
        format: FORMAT,
        kind: KIND.to_string(),
        generated_at: generated_at.to_string(),
        reason: reason.to_string(),
        supersedes,
        engine: labels.engine.clone(),
        machine: labels.machine.clone(),
        datasets: labels.datasets.clone(),
        behaviour: Behaviour {
            width,
            width_source: "artifact".to_string(),
            evaluator: reference::EVALUATOR_ID.to_string(),
            table: reference::EVALUATOR_TABLE.to_string(),
            table_fnv: reference::table_fnv(),
            toggles: reference::TOGGLES.to_string(),
            tail: true,
            tail_pieces: engine::search::TAIL,
        },
        split: measurements
            .first()
            .map(|measurement| measurement.split)
            .unwrap_or(Split::TEST),
        configs,
        reference: reference_rows.to_vec(),
    }
}

// ---------------------------------------------------------------- the compare

/// One seed whose measured value is not the golden's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mismatch {
    pub seed: u32,
    pub expected: u32,
    pub actual: u32,
}

/// The verdict on one vector: every mismatch kept, so the count in the failure
/// message is exact rather than "some seeds moved".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comparison {
    pub checked: usize,
    pub mismatches: Vec<Mismatch>,
}

impl Comparison {
    pub fn passes(&self) -> bool {
        self.mismatches.is_empty()
    }

    pub fn matched(&self) -> usize {
        self.checked - self.mismatches.len()
    }
}

/// Compare two per-seed vectors exactly. `first` is the split's first seed, so the
/// message names seeds rather than array offsets.
pub fn compare(expected: &[u32], actual: &[u32], first: u32) -> Comparison {
    let mut mismatches = Vec::new();
    for index in 0..expected.len().max(actual.len()) {
        let left = expected.get(index).copied();
        let right = actual.get(index).copied();
        if left != right {
            mismatches.push(Mismatch {
                seed: first + index as u32,
                expected: left.unwrap_or(0),
                actual: right.unwrap_or(0),
            });
        }
    }
    Comparison {
        checked: expected.len().max(actual.len()),
        mismatches,
    }
}

/// §9.3's wall-clock bound, normalised by the calibration slice (§4.6).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bound {
    /// `batch_ms / golden batch_ms`.
    pub raw: f64,
    /// `(batch_ms/cal_ms) / (golden batch_ms/golden cal_ms)`.
    pub normalised: f64,
    pub factor: f64,
}

impl Bound {
    pub fn passes(&self) -> bool {
        self.normalised <= self.factor
    }
}

/// The bound, exactly: the ratio of the two ratios, so a uniformly slow box passes
/// and a real regression does not.
pub fn bound(golden_batch_ms: f64, golden_cal_ms: f64, batch_ms: f64, cal_ms: f64) -> Bound {
    let raw = if golden_batch_ms > 0.0 {
        batch_ms / golden_batch_ms
    } else {
        0.0
    };
    let normalised = if cal_ms > 0.0 && golden_cal_ms > 0.0 {
        (batch_ms / cal_ms) / (golden_batch_ms / golden_cal_ms)
    } else {
        raw
    };
    Bound {
        raw,
        normalised,
        factor: WALL_CLOCK_FACTOR,
    }
}

// -------------------------------------------------------------- the identity

/// Which of the two — or which of the labels — moved, in words a reviewer can act
/// on. Empty means "this is not gate 6b's question; compare the seeds".
pub fn identity_diffs(golden: &Golden, identity: &Identity) -> (Vec<String>, Vec<String>) {
    let mut diffs = Vec::new();
    let mut notes = Vec::new();

    for config in dataset::CONFIGS {
        let wanted = golden.dataset(config.dir);
        let actual = identity
            .datasets
            .iter()
            .find(|(name, _)| name == config.dir)
            .map(|(_, id)| id);
        match (wanted, actual) {
            (Some(wanted), Some(actual)) => {
                if wanted.fingerprint != actual.fingerprint {
                    diffs.push(format!(
                        "dataset fingerprint · {}: golden {} · actual {} — the games moved, so this is \
                         a Dataset change (a new generator version and a new Reference row), not a \
                         goldens comparison",
                        config.dir, wanted.fingerprint, actual.fingerprint
                    ));
                }
                if wanted.generator_version != actual.generator_version {
                    diffs.push(format!(
                        "dataset generator_version · {}: golden {} · actual {}",
                        config.dir, wanted.generator_version, actual.generator_version
                    ));
                }
            }
            (Some(_), None) => diffs.push(format!(
                "dataset · {}: the golden names it, this box does not",
                config.dir
            )),
            _ => {}
        }
    }

    if golden.behaviour.width != identity.behaviour.width {
        diffs.push(format!(
            "behaviour width · golden {} · actual {} — DEFAULT_BEAM is a golden number (§9.5): moving \
             it is one deliberate update across the curve, the Reference row and this file",
            golden.behaviour.width, identity.behaviour.width
        ));
    }
    if golden.behaviour.evaluator != identity.behaviour.evaluator
        || golden.behaviour.table != identity.behaviour.table
    {
        diffs.push(format!(
            "behaviour evaluator · golden {}/{} · actual {}/{} — §9.4's ladder names what ships",
            golden.behaviour.evaluator,
            golden.behaviour.table,
            identity.behaviour.evaluator,
            identity.behaviour.table
        ));
    }
    if golden.behaviour.toggles != identity.behaviour.toggles {
        diffs.push(format!(
            "behaviour toggles · golden {} · actual {}",
            golden.behaviour.toggles, identity.behaviour.toggles
        ));
    }

    if golden.engine.wasm_sha256 != identity.engine.wasm_sha256 {
        if os_family(&golden.machine.os) == os_family(&identity.machine.os) {
            diffs.push(format!(
                "engine hash · golden {} · actual {} — the artifact the page loads changed (a source \
                 change, a build-flag change or a different rustc), so the golden has to be re-pinned \
                 deliberately",
                golden.engine.wasm_sha256, identity.engine.wasm_sha256
            ));
        } else {
            // A module built on another OS embeds another separator, so its hash cannot
            // match this one on a perfect build (notes/09 §2.3). Reported, not asserted:
            // the identity then rests on the commit, the Dataset and the behaviour.
            notes.push(format!(
                "engine hash differs ({} vs {}) and this box is {}, not {} — a wasm module built on \
                 another OS carries different embedded paths, so the hash is reported rather than \
                 compared here",
                short_hash(&golden.engine.wasm_sha256),
                short_hash(&identity.engine.wasm_sha256),
                os_family(&identity.machine.os),
                os_family(&golden.machine.os)
            ));
        }
    }

    (diffs, notes)
}

fn os_family(os: &str) -> &str {
    let lower = os.to_ascii_lowercase();
    if lower.contains("windows") {
        "windows"
    } else if lower.contains("darwin") || lower.contains("macos") || lower.contains("mac os") {
        "macos"
    } else if lower.contains("linux") {
        "linux"
    } else {
        "unknown"
    }
}

fn short_hash(hash: &str) -> &str {
    if hash.len() > 12 {
        &hash[..12]
    } else {
        hash
    }
}

// ----------------------------------------------------------------- the gate

/// Gate 6b, as §9.2 and §9.6 describe it: identity first, then the two vectors
/// exactly, then the loose clock. The checks are built here rather than in
/// [`crate::gate`] so the failure messages live beside the comparisons that produce
/// them — and so `bench golden check` prints the same block CI does.
pub fn checks(golden: &Golden, identity: &Identity, measurements: &[Measurement]) -> Vec<Check> {
    let (diffs, notes) = identity_diffs(golden, identity);
    if !diffs.is_empty() {
        return vec![Check::fail("golden identity")
            .subject(format!(
                "{} identity field{} moved: the golden at {} was pinned on a different {}",
                diffs.len(),
                if diffs.len() == 1 { "" } else { "s" },
                golden.generated_at,
                if diffs.iter().any(|diff| diff.starts_with("engine hash")) {
                    "engine"
                } else if diffs.iter().any(|diff| diff.starts_with("dataset")) {
                    "Dataset"
                } else {
                    "behaviour"
                }
            ))
            .lines(diffs)
            .hint(
                "this is not gate 6b's question: a different identity *should* produce different \
                 numbers. Run the Reference run and re-pin the golden deliberately — `bench \
                 reference --append --reason \"…\"` then `bench golden update --reason \"…\"` — \
                 together with `data/measurements/width-curve.json` if the width moved (§9.5).",
            )];
    }

    let mut checks = vec![{
        let mut check = Check::pass(
            "golden identity",
            format!(
                "engine {} · wasm {} · width {} · evaluator {}/{}{}",
                golden.engine.short,
                short_hash(&identity.engine.wasm_sha256),
                identity.behaviour.width,
                identity.behaviour.evaluator,
                identity.behaviour.table,
                dataset_label(&identity.datasets)
            ),
        );
        if !notes.is_empty() {
            check = check.lines(notes);
        }
        check
    }];

    for config in dataset::CONFIGS {
        let entry = match golden.entry(config.dir) {
            Some(entry) => entry,
            None => {
                checks.push(Check::fail(format!("golden evaluations · {}", config.dir))
                    .subject("the golden has no entry for this configuration"));
                continue;
            }
        };
        let measurement = measurements.iter().find(|item| item.config.dir == config.dir);
        let Some(measurement) = measurement else {
            checks.push(Check::fail(format!("golden evaluations · {}", config.dir))
                .subject("no measurement was taken for this configuration"));
            continue;
        };

        checks.push(vector_check(
            "evals",
            config,
            golden.split.first,
            &entry.evals,
            &entry.evals_fnv1a64,
            &measurement.evals(),
            &measurement.evals_fingerprint(),
            "an Evaluation count",
            "Evaluations",
        ));
        checks.push(vector_check(
            "lines",
            config,
            golden.split.first,
            &entry.lines,
            &entry.lines_fnv1a64,
            &measurement.lines(),
            &measurement.lines_fingerprint(),
            "a Lines count",
            "Lines",
        ));
        checks.push(clock_check(config, &entry.stats, &measurement.stats()));
    }
    checks
}

fn dataset_label(datasets: &[(String, DatasetId)]) -> String {
    datasets
        .iter()
        .map(|(name, id)| format!(" · {name} {}", id.fingerprint))
        .collect::<String>()
}

/// One vector's check: the digest is the verdict, the rows are the diagnosis.
fn vector_check(
    what: &str,
    config: Config,
    first_seed: u32,
    expected: &[u32],
    expected_digest: &str,
    actual: &[u32],
    actual_digest: &str,
    subject: &str,
    noun: &str,
) -> Check {
    let name = format!("golden {what} · {}", config.dir);
    let comparison = compare(expected, actual, first_seed);
    if comparison.passes() {
        return Check::pass(
            name,
            format!(
                "{} of {} seeds, digest {}",
                comparison.checked, comparison.checked, expected_digest
            ),
        );
    }

    let mut lines = Vec::new();
    for (index, mismatch) in comparison.mismatches.iter().take(MAX_NAMED).enumerate() {
        let delta = mismatch.actual as i64 - mismatch.expected as i64;
        lines.push(format!(
            "{} · seed {} · expected {} · actual {} ({delta:+})",
            if index == 0 { "first mismatch" } else { "also" },
            mismatch.seed,
            mismatch.expected,
            mismatch.actual
        ));
    }
    let rest = comparison.mismatches.len().saturating_sub(MAX_NAMED);
    if rest > 0 {
        lines.push(format!("(+ {rest} more)"));
    }
    Check::fail(name)
        .subject(format!(
            "{} of {} seeds match; digest expected {expected_digest} actual {actual_digest}",
            comparison.matched(),
            comparison.checked
        ))
        .lines(lines)
        .hint(format!(
            "BEHAVIOUR CHANGE — {subject} is an integer from a deterministic search and cannot vary \
             with clock speed or load, and every seed is the same game as the golden's. Fix the \
             engine, or update the golden deliberately: `bench golden update --reason \"…\"` after a \
             Reference run appends its row (§9.6). The moved rows are {noun}, seed by seed."
        ))
}

fn clock_check(config: Config, golden: &Stats, measured: &Stats) -> Check {
    let name = format!("golden wall clock · {}", config.dir);
    let bound = bound(golden.batch_ms, golden.cal_ms, measured.batch_ms, measured.cal_ms);
    let subject = format!(
        "batch {:.0} ms vs golden {:.0} ms = {:.2}x raw; calibration {:.0} ms vs {:.0} ms ⇒ \
         normalised {:.2}x (bound {:.0}x)",
        measured.batch_ms,
        golden.batch_ms,
        bound.raw,
        measured.cal_ms,
        golden.cal_ms,
        bound.normalised,
        bound.factor
    );
    if bound.passes() {
        let check = if bound.raw > bound.factor {
            Check::warn(name, subject)
        } else {
            Check::pass(name, subject)
        };
        return check.lines([format!(
            "headroom: the box may be {:.1}x slower than the labelled one before this trips",
            bound.factor / bound.normalised.max(f64::MIN_POSITIVE)
        )]);
    }
    Check::fail(name)
        .subject(subject)
        .hint(
            "MACHINE, not a behaviour change — the evaluations and Lines vectors are identical, so \
             the work is identical and only the clock moved. This is not a red engine: re-run on a \
             quiet box, and if it persists on the labelled box the engine got slower and §9.3's \
             budget is the thing to look at, not the golden (ADR 0002).",
        )
}

/// The state a golden check would report, without a `Report`: for a caller that
/// wants the verdict rather than the block.
pub fn failed(checks: &[Check]) -> bool {
    checks.iter().any(|check| check.state == State::Fail)
}

/// Where the golden lives, next to the other measurements.
pub fn path() -> PathBuf {
    reference::golden_path()
}
