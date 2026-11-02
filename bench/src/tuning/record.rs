//! `data/measurements/e2-tuning.json` and `e2-tuning-rows.jsonl`: the schema, the
//! renderer and the reader.
//!
//! Hand-rolled, because `bench` has no dependencies by design (`run/log.md`) and
//! the record is one document in one shape. The renderer is the writer; the
//! reader is what `bench tune --replay`, the append-only retry path and
//! `bench/tests/tuning_record.rs` use to hold the committed file to what it says.
//!
//! Two fields carry the ticket's honesty requirements and are deliberately the
//! first two a reader meets after the run number:
//!
//! * `decision` — `"accepted"` or `"rejected"`;
//! * `rejection_kind` / `rejection_reason` — `"worse_than_published"`,
//!   `"no_improvement"` or `"budget"`, and one sentence naming the numbers.
//!
//! A run is **never** deleted, so the file is the history of how many attempts the
//! table took, and a top-level `accepted: null` is the one field that says "no
//! table here cleared the rule".

use crate::json::Value;
use crate::tuning::stats::Stats;

/// The schema string. Bumping it is a deliberate reader change.
pub const SCHEMA: &str = "e2-tuning/1";

/// `data/measurements/e2-tuning.json`, from the Dataset root: the tree §9.5
/// already created for the width curve.
pub fn record_path() -> std::path::PathBuf {
    crate::dataset::root()
        .parent()
        .expect("data/dataset has a parent: data/")
        .join("measurements")
        .join("e2-tuning.json")
}

/// `data/measurements/e2-tuning-rows.jsonl`: the per-seed rows ticket 08 compares.
pub fn rows_path() -> std::path::PathBuf {
    crate::dataset::root()
        .parent()
        .expect("data/dataset has a parent: data/")
        .join("measurements")
        .join("e2-tuning-rows.jsonl")
}

// ------------------------------------------------------------------- the shape

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BedName {
    pub name: String,
    pub goal: u32,
    pub pieces: u32,
    pub cap: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WidthCurveRef {
    pub path: String,
    pub default_beam: u32,
    pub default_beam_basis: String,
    pub quality_fingerprint: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prng {
    pub algorithm: String,
    pub stream_xor: String,
    pub bootstrap_xor: String,
    pub bounded: String,
    pub normal: String,
    pub draw_order: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct E2VsE1 {
    /// E1 wins where E2 loses.
    pub b: u32,
    /// E2 wins where E1 loses.
    pub c: u32,
    pub mcnemar_p: f64,
    pub win_rate_delta: f64,
    pub mean_lines_delta: f64,
    pub mean_lines_ci95: [f64; 2],
    pub evals_median_delta: f64,
    pub evals_p90_delta: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Comparison {
    pub e2_vs_e1: E2VsE1,
    /// `"resolved"` when the bed can rank on win rate, `"no-resolution"` when
    /// both tables win ≥ 99 % and only the tie-breaks separate them — the same
    /// vocabulary the width curve uses for its quality axis.
    pub win_rate_axis: String,
    /// The lexicographic rule the decision used, quoted so it cannot be re-read
    /// after the fact.
    pub rule: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Machine {
    pub engine_commit: String,
    pub bench_commit: String,
    pub rustc: String,
    pub target: String,
    pub cpu: String,
    pub os: String,
    pub profile: String,
    pub wall_clock_seconds: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub run: u32,
    pub date: String,
    pub decision: String,
    pub rejection_kind: Option<String>,
    pub rejection_reason: Option<String>,
    pub bed: BedName,
    pub bed_source: String,
    pub dataset_fingerprint: String,
    pub generator_version: u32,
    pub width: u32,
    pub width_curve: WidthCurveRef,
    pub split: String,
    pub seed_list_fnv: String,
    pub minibatch_fnv: String,
    pub tune_seed: String,
    pub prng: Prng,
    /// The CE hyper-parameters, rendered from the run's own [`CeParams`]: a
    /// hyper-parameter that is not in the record is a hyper-parameter nobody can
    /// reproduce.
    pub params: Vec<(String, String)>,
    pub iterations_run: u32,
    pub stop_reason: String,
    pub weights_decimal: [f64; 8],
    pub weights_q16: [i32; 8],
    pub table_fnv: String,
    /// `e0`, `e1`, `bcts`, `e2` — every table this run measured on the tuning
    /// Split, so ticket 08 compares instead of retraining.
    pub stats: Vec<(String, Stats)>,
    pub comparison: Comparison,
    pub machine: Machine,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accepted {
    pub table: String,
    pub run: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub runs: Vec<Run>,
    pub accepted: Option<Accepted>,
}

/// One `(run, evaluator, seed)` line of `e2-tuning-rows.jsonl`.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub run: u32,
    pub evaluator: String,
    pub table: String,
    pub bed: String,
    pub width: u32,
    pub seed: u32,
    pub won: bool,
    pub lines: u32,
    pub evals: u32,
    pub ms: f64,
    pub moves: u32,
    pub dataset_fingerprint: String,
}

// ------------------------------------------------------------------ rendering

struct Out {
    text: String,
    depth: usize,
}

impl Out {
    fn new() -> Out {
        Out {
            text: String::new(),
            depth: 0,
        }
    }

    fn indent(&mut self) {
        for _ in 0..self.depth {
            self.text.push_str("  ");
        }
    }

    fn open(&mut self, key: &str, brace: char) {
        self.indent();
        if !key.is_empty() {
            self.text.push('"');
            self.text.push_str(key);
            self.text.push_str("\": ");
        }
        self.text.push(brace);
        self.text.push('\n');
        self.depth += 1;
    }

    fn close(&mut self, brace: char, comma: bool) {
        self.depth -= 1;
        self.indent();
        self.text.push(brace);
        if comma {
            self.text.push(',');
        }
        self.text.push('\n');
    }

    fn field(&mut self, key: &str, value: &str) {
        self.indent();
        self.text.push('"');
        self.text.push_str(key);
        self.text.push_str("\": ");
        self.text.push_str(value);
        self.text.push_str(",\n");
    }

    fn tail(&mut self, key: &str, value: &str) {
        self.indent();
        self.text.push('"');
        self.text.push_str(key);
        self.text.push_str("\": ");
        self.text.push_str(value);
        self.text.push('\n');
    }
}

fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// The record's number format, shared with `CeParams::render` so a
/// hyper-parameter and a statistic print the same way.
pub fn number_for(value: f64) -> String {
    number(value)
}

fn number(value: f64) -> String {
    if !value.is_finite() {
        return "null".to_string();
    }
    // Six decimals is the record's resolution for a decimal weight and more than
    // enough for a rate; trimming keeps the file readable.
    let mut text = format!("{value:.6}");
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    if text == "-0" {
        text = "0".to_string();
    }
    text
}

fn opt(text: &Option<String>) -> String {
    match text {
        Some(value) => quote(value),
        None => "null".to_string(),
    }
}

fn numbers_f64(values: &[f64]) -> String {
    let parts: Vec<String> = values.iter().map(|value| number(*value)).collect();
    format!("[{}]", parts.join(", "))
}

fn numbers_i32(values: &[i32]) -> String {
    let parts: Vec<String> = values.iter().map(|value| value.to_string()).collect();
    format!("[{}]", parts.join(", "))
}

impl Stats {
    /// `last` is the object's final field: a trailing comma would make the record
    /// unparseable, which is exactly what `bench tune --replay` caught the first
    /// time this file was written.
    fn render_fields(&self, out: &mut Out, last: bool) {
        out.field("wins", &self.wins.to_string());
        out.field("games", &self.games.to_string());
        out.field("win_rate", &number(self.win_rate));
        out.field("win_rate_ci95", &numbers_f64(&self.win_rate_ci95));
        out.field("mean_lines", &number(self.mean_lines));
        out.field("mean_lines_on_wins", &number(self.mean_lines_on_wins));
        out.field("mean_pieces_saved", &number(self.mean_pieces_saved));
        out.field("evals_mean", &number(self.evals_mean));
        out.field("evals_median", &number(self.evals_median));
        out.field("evals_p90", &number(self.evals_p90));
        out.field("ms_median", &number(self.ms_median));
        if last {
            out.tail("ms_p90", &number(self.ms_p90));
        } else {
            out.field("ms_p90", &number(self.ms_p90));
        }
    }
}

impl Run {
    fn render(&self, out: &mut Out, last: bool) {
        out.open("", '{');
        out.field("run", &self.run.to_string());
        out.field("date", &quote(&self.date));
        // The decision and its reason first: they are what a reader must not miss.
        out.field("decision", &quote(&self.decision));
        out.field("rejection_kind", &opt(&self.rejection_kind));
        out.field("rejection_reason", &opt(&self.rejection_reason));

        out.open("bed", '{');
        out.field("name", &quote(&self.bed.name));
        out.field("goal", &self.bed.goal.to_string());
        out.field("pieces", &self.bed.pieces.to_string());
        out.tail("cap", &self.bed.cap.to_string());
        out.close('}', true);

        out.field("bed_source", &quote(&self.bed_source));
        out.field("dataset_fingerprint", &quote(&self.dataset_fingerprint));
        out.field("generator_version", &self.generator_version.to_string());
        out.field("width", &self.width.to_string());

        out.open("width_curve", '{');
        out.field("path", &quote(&self.width_curve.path));
        out.field("default_beam", &self.width_curve.default_beam.to_string());
        out.field(
            "default_beam_basis",
            &quote(&self.width_curve.default_beam_basis),
        );
        out.tail(
            "quality_fingerprint",
            &quote(&self.width_curve.quality_fingerprint),
        );
        out.close('}', true);

        out.field("split", &quote(&self.split));
        out.field("seed_list_fnv", &quote(&self.seed_list_fnv));
        out.field("minibatch_fnv", &quote(&self.minibatch_fnv));
        out.field("tune_seed", &quote(&self.tune_seed));

        out.open("prng", '{');
        out.field("algorithm", &quote(&self.prng.algorithm));
        out.field("stream_xor", &quote(&self.prng.stream_xor));
        out.field("bootstrap_xor", &quote(&self.prng.bootstrap_xor));
        out.field("bounded", &quote(&self.prng.bounded));
        out.field("normal", &quote(&self.prng.normal));
        out.tail("draw_order", &quote(&self.prng.draw_order));
        out.close('}', true);

        out.open("params", '{');
        let count = self.params.len();
        for (index, (key, value)) in self.params.iter().enumerate() {
            if index + 1 == count {
                out.tail(key, value);
            } else {
                out.field(key, value);
            }
        }
        out.close('}', true);

        out.field("iterations_run", &self.iterations_run.to_string());
        out.field("stop_reason", &quote(&self.stop_reason));
        out.field("weights_decimal", &numbers_f64(&self.weights_decimal));
        out.field("weights_q16", &numbers_i32(&self.weights_q16));
        out.field("table_fnv", &quote(&self.table_fnv));

        out.open("tables", '{');
        let count = self.stats.len();
        for (index, (name, stats)) in self.stats.iter().enumerate() {
            out.indent();
            out.text.push('"');
            out.text.push_str(name);
            out.text.push_str("\": {\n");
            out.depth += 1;
            stats.render_fields(out, true);
            out.depth -= 1;
            out.indent();
            out.text.push('}');
            if index + 1 < count {
                out.text.push(',');
            }
            out.text.push('\n');
        }
        out.close('}', true);

        out.open("comparison", '{');
        out.field("win_rate_axis", &quote(&self.comparison.win_rate_axis));
        out.field("rule", &quote(&self.comparison.rule));
        out.open("e2_vs_e1", '{');
        let c = &self.comparison.e2_vs_e1;
        out.field("b", &c.b.to_string());
        out.field("c", &c.c.to_string());
        out.field("n", &(c.b + c.c).to_string());
        out.field("mcnemar_p", &number(c.mcnemar_p));
        out.field("win_rate_delta", &number(c.win_rate_delta));
        out.field("mean_lines_delta", &number(c.mean_lines_delta));
        out.field("mean_lines_ci95", &numbers_f64(&c.mean_lines_ci95));
        out.field("evals_median_delta", &number(c.evals_median_delta));
        out.tail("evals_p90_delta", &number(c.evals_p90_delta));
        out.close('}', false);
        out.close('}', true);

        out.open("machine", '{');
        out.field("engine_commit", &quote(&self.machine.engine_commit));
        out.field("bench_commit", &quote(&self.machine.bench_commit));
        out.field("rustc", &quote(&self.machine.rustc));
        out.field("target", &quote(&self.machine.target));
        out.field("cpu", &quote(&self.machine.cpu));
        out.field("os", &quote(&self.machine.os));
        out.field("profile", &quote(&self.machine.profile));
        out.tail(
            "wall_clock_seconds",
            &self.machine.wall_clock_seconds.to_string(),
        );
        out.close('}', false);
        out.close('}', !last);
    }
}

impl Record {
    pub fn render(&self) -> String {
        let mut out = Out::new();
        out.open("", '{');
        out.field("schema", &quote(SCHEMA));
        out.open("runs", '[');
        let count = self.runs.len();
        for (index, run) in self.runs.iter().enumerate() {
            run.render(&mut out, index + 1 == count);
        }
        out.close(']', true);
        match &self.accepted {
            Some(accepted) => {
                out.open("accepted", '{');
                out.field("table", &quote(&accepted.table));
                out.tail("run", &accepted.run.to_string());
                out.close('}', false);
            }
            None => out.tail("accepted", "null"),
        }
        out.close('}', false);
        out.text
    }

    // ---------------------------------------------------------------- reading

    pub fn parse(text: &str) -> Result<Record, String> {
        let value = crate::json::parse(text)?;
        let schema = text_of(&value, "schema")?;
        if schema != SCHEMA {
            return Err(format!("schema: expected {SCHEMA:?}, found {schema:?}"));
        }
        let runs = match value.get("runs").and_then(Value::as_array) {
            Some(runs) => runs,
            None => return Err("runs: expected an array".to_string()),
        };
        let mut parsed = Vec::with_capacity(runs.len());
        for run in runs {
            parsed.push(parse_run(run)?);
        }
        let accepted = match value.get("accepted") {
            Some(Value::Null) | None => None,
            Some(entry) => Some(Accepted {
                table: text_of(entry, "table")?,
                run: count_of(entry, "run")?,
            }),
        };
        Ok(Record {
            runs: parsed,
            accepted,
        })
    }
}

fn parse_run(value: &Value) -> Result<Run, String> {
    let bed = field(value, "bed")?;
    let width_curve = field(value, "width_curve")?;
    let prng = field(value, "prng")?;
    let comparison = field(value, "comparison")?;
    let machine = field(value, "machine")?;

    let weights_decimal = array_of_f64(value, "weights_decimal")?;
    let weights_q16 = array_of_i32(value, "weights_q16")?;

    let tables = field(value, "tables")?;
    let mut stats = Vec::new();
    for name in ["e0", "e1", "bcts", "e2"] {
        if let Some(entry) = tables.get(name) {
            stats.push((name.to_string(), parse_stats(entry)?));
        }
    }

    let params = match field(value, "params")?.as_object() {
        Some(fields) => fields
            .iter()
            .map(|(key, value)| Ok((key.clone(), render_value(value)?)))
            .collect::<Result<Vec<_>, String>>()?,
        None => return Err("params: expected an object".to_string()),
    };

    Ok(Run {
        run: count_of(value, "run")?,
        date: text_of(value, "date")?,
        decision: text_of(value, "decision")?,
        rejection_kind: optional_text(value, "rejection_kind")?,
        rejection_reason: optional_text(value, "rejection_reason")?,
        bed: BedName {
            name: text_of(bed, "name")?,
            goal: count_of(bed, "goal")?,
            pieces: count_of(bed, "pieces")?,
            cap: count_of(bed, "cap")?,
        },
        bed_source: text_of(value, "bed_source")?,
        dataset_fingerprint: text_of(value, "dataset_fingerprint")?,
        generator_version: count_of(value, "generator_version")?,
        width: count_of(value, "width")?,
        width_curve: WidthCurveRef {
            path: text_of(width_curve, "path")?,
            default_beam: count_of(width_curve, "default_beam")?,
            default_beam_basis: text_of(width_curve, "default_beam_basis")?,
            quality_fingerprint: text_of(width_curve, "quality_fingerprint")?,
        },
        split: text_of(value, "split")?,
        seed_list_fnv: text_of(value, "seed_list_fnv")?,
        minibatch_fnv: text_of(value, "minibatch_fnv")?,
        tune_seed: text_of(value, "tune_seed")?,
        prng: Prng {
            algorithm: text_of(prng, "algorithm")?,
            stream_xor: text_of(prng, "stream_xor")?,
            bootstrap_xor: text_of(prng, "bootstrap_xor")?,
            bounded: text_of(prng, "bounded")?,
            normal: text_of(prng, "normal")?,
            draw_order: text_of(prng, "draw_order")?,
        },
        params,
        iterations_run: count_of(value, "iterations_run")?,
        stop_reason: text_of(value, "stop_reason")?,
        weights_decimal,
        weights_q16,
        table_fnv: text_of(value, "table_fnv")?,
        stats,
        comparison: Comparison {
            e2_vs_e1: E2VsE1 {
                b: count_of(field(comparison, "e2_vs_e1")?, "b")?,
                c: count_of(field(comparison, "e2_vs_e1")?, "c")?,
                mcnemar_p: number_of(field(comparison, "e2_vs_e1")?, "mcnemar_p")?,
                win_rate_delta: number_of(field(comparison, "e2_vs_e1")?, "win_rate_delta")?,
                mean_lines_delta: number_of(field(comparison, "e2_vs_e1")?, "mean_lines_delta")?,
                mean_lines_ci95: pair_of_f64(field(comparison, "e2_vs_e1")?, "mean_lines_ci95")?,
                evals_median_delta: number_of(
                    field(comparison, "e2_vs_e1")?,
                    "evals_median_delta",
                )?,
                evals_p90_delta: number_of(field(comparison, "e2_vs_e1")?, "evals_p90_delta")?,
            },
            win_rate_axis: text_of(comparison, "win_rate_axis")?,
            rule: text_of(comparison, "rule")?,
        },
        machine: Machine {
            engine_commit: text_of(machine, "engine_commit")?,
            bench_commit: text_of(machine, "bench_commit")?,
            rustc: text_of(machine, "rustc")?,
            target: text_of(machine, "target")?,
            cpu: text_of(machine, "cpu")?,
            os: text_of(machine, "os")?,
            profile: text_of(machine, "profile")?,
            wall_clock_seconds: count_of(machine, "wall_clock_seconds")? as u64,
        },
    })
}

fn parse_stats(value: &Value) -> Result<Stats, String> {
    Ok(Stats {
        wins: count_of(value, "wins")?,
        games: count_of(value, "games")?,
        win_rate: number_of(value, "win_rate")?,
        win_rate_ci95: pair_of_f64(value, "win_rate_ci95")?,
        mean_lines: number_of(value, "mean_lines")?,
        mean_lines_on_wins: number_of(value, "mean_lines_on_wins")?,
        mean_pieces_saved: number_of(value, "mean_pieces_saved")?,
        evals_mean: number_of(value, "evals_mean")?,
        evals_median: number_of(value, "evals_median")?,
        evals_p90: number_of(value, "evals_p90")?,
        ms_median: number_of(value, "ms_median")?,
        ms_p90: number_of(value, "ms_p90")?,
    })
}

impl Row {
    pub fn render(&self) -> String {
        format!(
            "{{\"run\":{},\"evaluator\":{},\"table\":{},\"bed\":{},\"width\":{},\"seed\":{},\"won\":{},\"lines\":{},\"evals\":{},\"ms\":{},\"moves\":{},\"dataset_fingerprint\":{}}}",
            self.run,
            quote(&self.evaluator),
            quote(&self.table),
            quote(&self.bed),
            self.width,
            self.seed,
            self.won,
            self.lines,
            self.evals,
            number(self.ms),
            self.moves,
            quote(&self.dataset_fingerprint),
        )
    }

    pub fn parse(line: &str) -> Result<Row, String> {
        let value = crate::json::parse(line)?;
        Ok(Row {
            run: count_of(&value, "run")?,
            evaluator: text_of(&value, "evaluator")?,
            table: text_of(&value, "table")?,
            bed: text_of(&value, "bed")?,
            width: count_of(&value, "width")?,
            seed: count_of(&value, "seed")?,
            won: value
                .get("won")
                .and_then(Value::as_bool)
                .ok_or_else(|| "won: expected a boolean".to_string())?,
            lines: count_of(&value, "lines")?,
            evals: count_of(&value, "evals")?,
            ms: number_of(&value, "ms")?,
            moves: count_of(&value, "moves")?,
            dataset_fingerprint: text_of(&value, "dataset_fingerprint")?,
        })
    }
}

// ------------------------------------------------------------------- readers

fn field<'a>(value: &'a Value, key: &str) -> Result<&'a Value, String> {
    value.get(key).ok_or_else(|| format!("{key}: missing"))
}

fn text_of(value: &Value, key: &str) -> Result<String, String> {
    field(value, key)?
        .as_str()
        .map(|text| text.to_string())
        .ok_or_else(|| format!("{key}: expected a string"))
}

fn optional_text(value: &Value, key: &str) -> Result<Option<String>, String> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(entry) => entry
            .as_str()
            .map(|text| Some(text.to_string()))
            .ok_or_else(|| format!("{key}: expected a string or null")),
    }
}

fn count_of(value: &Value, key: &str) -> Result<u32, String> {
    field(value, key)?
        .as_u32()
        .ok_or_else(|| format!("{key}: expected a non-negative integer"))
}

fn number_of(value: &Value, key: &str) -> Result<f64, String> {
    field(value, key)?
        .as_f64()
        .ok_or_else(|| format!("{key}: expected a number"))
}

fn pair_of_f64(value: &Value, key: &str) -> Result<[f64; 2], String> {
    let array = field(value, key)?
        .as_array()
        .ok_or_else(|| format!("{key}: expected an array"))?;
    if array.len() != 2 {
        return Err(format!(
            "{key}: expected two numbers, found {}",
            array.len()
        ));
    }
    Ok([
        array[0]
            .as_f64()
            .ok_or_else(|| format!("{key}[0]: expected a number"))?,
        array[1]
            .as_f64()
            .ok_or_else(|| format!("{key}[1]: expected a number"))?,
    ])
}

fn array_of_f64(value: &Value, key: &str) -> Result<[f64; 8], String> {
    let array = field(value, key)?
        .as_array()
        .ok_or_else(|| format!("{key}: expected an array"))?;
    if array.len() != 8 {
        return Err(format!(
            "{key}: expected eight numbers, found {}",
            array.len()
        ));
    }
    let mut out = [0.0f64; 8];
    for (index, entry) in array.iter().enumerate() {
        out[index] = entry
            .as_f64()
            .ok_or_else(|| format!("{key}[{index}]: expected a number"))?;
    }
    Ok(out)
}

fn array_of_i32(value: &Value, key: &str) -> Result<[i32; 8], String> {
    let array = field(value, key)?
        .as_array()
        .ok_or_else(|| format!("{key}: expected an array"))?;
    if array.len() != 8 {
        return Err(format!(
            "{key}: expected eight numbers, found {}",
            array.len()
        ));
    }
    let mut out = [0i32; 8];
    for (index, entry) in array.iter().enumerate() {
        let number = entry
            .as_f64()
            .ok_or_else(|| format!("{key}[{index}]: expected a number"))?;
        out[index] = number as i32;
    }
    Ok(out)
}

fn render_value(value: &Value) -> Result<String, String> {
    Ok(match value {
        Value::Null => "null".to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => self::number(*number),
        Value::String(text) => quote(text),
        Value::Array(items) => {
            let parts: Vec<String> = items
                .iter()
                .map(render_value)
                .collect::<Result<Vec<_>, String>>()?;
            format!("[{}]", parts.join(", "))
        }
        Value::Object(fields) => {
            let parts: Vec<String> = fields
                .iter()
                .map(|(key, value)| Ok(format!("{}: {}", quote(key), render_value(value)?)))
                .collect::<Result<Vec<_>, String>>()?;
            format!("{{{}}}", parts.join(", "))
        }
    })
}
