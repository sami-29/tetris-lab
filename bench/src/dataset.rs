//! The Dataset's on-disk formats (§6): the header line, one line per game, and
//! `manifest.json`.
//!
//! Everything here is a *format*, not a policy: the two configurations, the
//! split ranges, the render/parse pair for each file, and one place that knows
//! where the files live (`data/dataset/`, resolved from this crate's own
//! directory so `cargo run -p bench` works from any working directory).

use std::path::{Path, PathBuf};

/// §6: the `1` after "dataset" is the *format* version, frozen. `generator_version`
/// lives in the manifest and is what §6.2 bumps.
pub const FORMAT: u32 = 1;

/// §6: seeds 0-999 are the test split, 1,000-1,999 the tuning split. Both
/// ranges live in every file.
pub const SEED_FIRST: u32 = 0;
pub const SEED_LAST: u32 = 1999;
pub const GAME_COUNT: usize = 2000;
pub const TEST_SPLIT: (u32, u32) = (0, 999);
pub const TUNING_SPLIT: (u32, u32) = (1000, 1999);

/// One shipped configuration: a name, and the three numbers the gate checks the
/// name against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    /// The directory under `data/dataset/`.
    pub dir: &'static str,
    pub goal: u32,
    pub pieces: u32,
    pub cap: u32,
}

impl Config {
    pub fn from_name(name: &str) -> Option<Config> {
        CONFIGS.iter().copied().find(|c| c.dir == name)
    }
}

pub const GOAL8: Config = Config {
    dir: "goal8-p40-cap7",
    goal: 8,
    pieces: 40,
    cap: 7,
};

pub const GOAL12: Config = Config {
    dir: "goal12-p100-cap7",
    goal: 12,
    pieces: 100,
    cap: 7,
};

pub const CONFIGS: [Config; 2] = [GOAL8, GOAL12];

/// The three committed files of a configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Games,
    Manifest,
    Reference,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Games, Kind::Manifest, Kind::Reference];

    pub fn file(self) -> &'static str {
        match self {
            Kind::Games => "games.txt",
            Kind::Manifest => "manifest.json",
            Kind::Reference => "reference.jsonl",
        }
    }
}

/// The one constant: the Dataset root, resolved from `bench/Cargo.toml`'s
/// directory. Every path this crate touches is built from it.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("bench/ always has a parent: the workspace root")
        .join("data")
        .join("dataset")
}

pub fn config_dir(data: &Path, cfg: &Config) -> PathBuf {
    data.join(cfg.dir)
}

pub fn path(data: &Path, cfg: &Config, kind: Kind) -> PathBuf {
    config_dir(data, cfg).join(kind.file())
}

pub fn games_path(data: &Path, cfg: &Config) -> PathBuf {
    path(data, cfg, Kind::Games)
}

pub fn manifest_path(data: &Path, cfg: &Config) -> PathBuf {
    path(data, cfg, Kind::Manifest)
}

pub fn reference_path(data: &Path, cfg: &Config) -> PathBuf {
    path(data, cfg, Kind::Reference)
}

/// §6.2: the previous file, under its own fingerprint, kept forever.
pub fn archive_dir(data: &Path, cfg: &Config) -> PathBuf {
    config_dir(data, cfg).join("archive")
}

// ---------------------------------------------------------------------- one game

/// One game as the file stores it: the seed, the ten column words and the
/// sequence letters. The same shape is built from the engine and from a parsed
/// line, which is what lets the gate compare them field by field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameLine {
    pub seed: u32,
    pub board: [u32; 10],
    pub sequence: Vec<u8>,
}

impl GameLine {
    /// From the engine's own `Game` -- no re-encoding, no re-placement.
    pub fn from_engine(game: &engine::Game) -> GameLine {
        GameLine {
            seed: game.seed,
            board: game.board.0,
            sequence: game.sequence.letters(),
        }
    }

    /// The Board the words describe, for the solver's own reachability tests.
    pub fn board(&self) -> engine::Board {
        engine::Board(self.board)
    }
}

// ---------------------------------------------------------------------- header

/// `# tetris-lab dataset 1 goal=8 pieces=40 cap=7 seeds=0-1999`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub format: u32,
    pub goal: u32,
    pub pieces: u32,
    pub cap: u32,
    pub first_seed: u32,
    pub last_seed: u32,
}

impl Header {
    pub fn new(cfg: &Config) -> Header {
        Header {
            format: FORMAT,
            goal: cfg.goal,
            pieces: cfg.pieces,
            cap: cfg.cap,
            first_seed: SEED_FIRST,
            last_seed: SEED_LAST,
        }
    }

    /// The whole line, so a stray extra field is a failure rather than an
    /// unknown token.
    pub fn canonical(&self) -> String {
        format!(
            "# tetris-lab dataset {} goal={} pieces={} cap={} seeds={}-{}",
            self.format, self.goal, self.pieces, self.cap, self.first_seed, self.last_seed
        )
    }
}

/// The canonical header line for a configuration.
pub fn header_line(cfg: &Config) -> String {
    Header::new(cfg).canonical()
}

fn parse_header(line: &str) -> Result<Header, String> {
    // Tolerant of runs of whitespace: the *values* are what matter here. The
    // gate separately demands that the header be the canonical line, byte for
    // byte, so a reformatted header is still caught -- once, in one place.
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() != 8 {
        return Err(format!(
            "header has {} space-separated fields, expected 8: {line:?}",
            fields.len()
        ));
    }
    if fields[0] != "#" || fields[1] != "tetris-lab" || fields[2] != "dataset" {
        return Err(format!(
            "header does not start with `# tetris-lab dataset`: {line:?}"
        ));
    }
    let format = number(fields[3], "format")?;
    let goal = keyed(fields[4], "goal")?;
    let pieces = keyed(fields[5], "pieces")?;
    let cap = keyed(fields[6], "cap")?;
    let seeds = fields[7].strip_prefix("seeds=").ok_or_else(|| {
        format!(
            "header field 8 is not `seeds=<first>-<last>`: {:?}",
            fields[7]
        )
    })?;
    let (first, last) = seeds
        .split_once('-')
        .ok_or_else(|| format!("header seed range is not `<first>-<last>`: {seeds:?}"))?;
    Ok(Header {
        format,
        goal,
        pieces,
        cap,
        first_seed: number(first, "seeds first")?,
        last_seed: number(last, "seeds last")?,
    })
}

fn number(text: &str, what: &str) -> Result<u32, String> {
    text.parse::<u32>()
        .map_err(|_| format!("{what} is not a decimal number: {text:?}"))
}

fn keyed(text: &str, key: &str) -> Result<u32, String> {
    let value = text
        .strip_prefix(key)
        .and_then(|rest| rest.strip_prefix('='))
        .ok_or_else(|| format!("header field is not `{key}=<number>`: {text:?}"))?;
    number(value, key)
}

// ------------------------------------------------------------------ games.txt

/// A parsed `games.txt`: the header it declares and its games in file order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed {
    pub header: Header,
    /// The header line exactly as it appears, so the caller can compare it to
    /// the canonical rendering.
    pub header_line: String,
    pub games: Vec<GameLine>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// 1-based line number in the file.
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

/// One game line: seed, ten 8-digit hex column words, then the letters.
pub fn game_line(game: &GameLine) -> String {
    let mut out = String::with_capacity(16 + 10 * 9 + game.sequence.len());
    out.push_str(&game.seed.to_string());
    for word in &game.board {
        out.push(' ');
        out.push_str(&format!("{word:08x}"));
    }
    out.push(' ');
    for letter in &game.sequence {
        out.push(*letter as char);
    }
    out
}

/// The whole file: one header line, one line per game in seed order, LF after
/// every line including the last. No alignment, no wrapping, no pretty printing
/// (§6: that is not the committed form).
pub fn render(cfg: &Config, games: &[GameLine]) -> String {
    let mut out = String::with_capacity(64 + games.len() * (140 + cfg.pieces as usize));
    out.push_str(&header_line(cfg));
    out.push('\n');
    for game in games {
        out.push_str(&game_line(game));
        out.push('\n');
    }
    out
}

/// Parse a `games.txt`. Semantic, not textual: the bytes may be reformatted
/// (CRLF, uppercase hex, extra spaces) and the games are still the same games.
/// `cfg` supplies the expected sequence length; the header is read, not assumed.
pub fn parse(cfg: &Config, text: &str) -> Result<Parsed, ParseError> {
    let mut lines = text.lines().enumerate();
    let (_, first) = lines.next().ok_or_else(|| ParseError {
        line: 1,
        message: "the file is empty".into(),
    })?;
    let header = parse_header(first).map_err(|message| ParseError { line: 1, message })?;
    let header_line = first.to_string();

    let mut games: Vec<GameLine> = Vec::with_capacity(GAME_COUNT);
    let mut previous: Option<u32> = None;
    for (index, line) in lines {
        let number = index + 1;
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() != 12 {
            return Err(ParseError {
                line: number,
                message: format!(
                    "expected 12 fields (seed, ten column words, sequence), found {}: {:?}",
                    fields.len(),
                    line
                ),
            });
        }
        let seed = fields[0].parse::<u32>().map_err(|_| ParseError {
            line: number,
            message: format!("seed is not a decimal u32: {:?}", fields[0]),
        })?;
        if let Some(previous) = previous {
            if seed <= previous {
                return Err(ParseError {
                    line: number,
                    message: format!("seeds must ascend: {seed} follows {previous}"),
                });
            }
            if seed != previous + 1 {
                return Err(ParseError {
                    line: number,
                    message: format!("seed {previous} is missing (the file jumps to {seed})"),
                });
            }
        }
        previous = Some(seed);

        let mut board = [0u32; 10];
        for (column, word) in fields[1..11].iter().enumerate() {
            if word.len() != 8 {
                return Err(ParseError {
                    line: number,
                    message: format!(
                        "column word {column} must be exactly 8 hex digits, found {:?}",
                        word
                    ),
                });
            }
            board[column] = u32::from_str_radix(word, 16).map_err(|_| ParseError {
                line: number,
                message: format!("column word {column} is not hexadecimal: {:?}", word),
            })?;
        }

        let letters = fields[11].as_bytes();
        if let Some(bad) = letters.iter().find(|b| !b"IJLOSTZ".contains(b)) {
            return Err(ParseError {
                line: number,
                message: format!(
                    "sequence letter {:?} is not one of I, J, L, O, S, T, Z",
                    *bad as char
                ),
            });
        }
        if letters.len() != cfg.pieces as usize {
            return Err(ParseError {
                line: number,
                message: format!(
                    "sequence has {} letters, but the configuration has {} pieces",
                    letters.len(),
                    cfg.pieces
                ),
            });
        }

        games.push(GameLine {
            seed,
            board,
            sequence: letters.to_vec(),
        });
    }

    Ok(Parsed {
        header,
        header_line,
        games,
    })
}

// ---------------------------------------------------------------- manifest.json

/// `manifest.json`: the configuration, both splits, the generator's identity and
/// the Dataset fingerprint (§6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub format: u32,
    pub goal: u32,
    pub pieces: u32,
    pub cap: u32,
    pub seeds: (u32, u32),
    pub test_split: (u32, u32),
    pub tuning_split: (u32, u32),
    pub game_count: u32,
    pub generator_version: u32,
    pub generator_description: String,
    /// 16 lowercase hex digits, a *string*: a u64 is not exact in JavaScript.
    pub fingerprint: String,
    /// §6.2's "reason in one manifest line", append-only, oldest first.
    pub notes: Vec<String>,
}

/// ADR 0003's first paragraph, in one line: what the manifest says the games are.
pub const GENERATOR_DESCRIPTION: &str = "splitmix64 from seed^C_fill (board) and seed^C_sequence (bag), pre-fill draws piece/rotation/column and stops before the crossing piece, 7-bags shuffled Fisher-Yates descending with an S/Z re-shuffle at the seam";

impl Manifest {
    /// A fresh manifest for a configuration, before any change: version 1, no
    /// notes, no fingerprint (the exporter fills it).
    pub fn for_config(cfg: &Config, fingerprint: &str) -> Manifest {
        Manifest {
            format: FORMAT,
            goal: cfg.goal,
            pieces: cfg.pieces,
            cap: cfg.cap,
            seeds: (SEED_FIRST, SEED_LAST),
            test_split: TEST_SPLIT,
            tuning_split: TUNING_SPLIT,
            game_count: GAME_COUNT as u32,
            generator_version: 1,
            generator_description: GENERATOR_DESCRIPTION.to_string(),
            fingerprint: fingerprint.to_string(),
            notes: Vec::new(),
        }
    }

    /// Pretty-printed with two spaces and a fixed key order: this is the one
    /// file in the Dataset a human reviews.
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(512);
        out.push_str("{\n");
        out.push_str(&format!("  \"format\": {},\n", self.format));
        out.push_str(&format!("  \"goal\": {},\n", self.goal));
        out.push_str(&format!("  \"pieces\": {},\n", self.pieces));
        out.push_str(&format!("  \"cap\": {},\n", self.cap));
        out.push_str(&format!(
            "  \"seeds\": {{ \"first\": {}, \"last\": {} }},\n",
            self.seeds.0, self.seeds.1
        ));
        out.push_str("  \"splits\": {\n");
        out.push_str(&format!(
            "    \"test\": {{ \"first\": {}, \"last\": {} }},\n",
            self.test_split.0, self.test_split.1
        ));
        out.push_str(&format!(
            "    \"tuning\": {{ \"first\": {}, \"last\": {} }}\n",
            self.tuning_split.0, self.tuning_split.1
        ));
        out.push_str("  },\n");
        out.push_str(&format!("  \"game_count\": {},\n", self.game_count));
        out.push_str(&format!(
            "  \"generator_version\": {},\n",
            self.generator_version
        ));
        out.push_str(&format!(
            "  \"generator_description\": {},\n",
            json_string(&self.generator_description)
        ));
        out.push_str(&format!(
            "  \"fingerprint\": {},\n",
            json_string(&self.fingerprint)
        ));
        if self.notes.is_empty() {
            out.push_str("  \"notes\": []\n");
        } else {
            out.push_str("  \"notes\": [\n");
            for (index, note) in self.notes.iter().enumerate() {
                out.push_str("    ");
                out.push_str(&json_string(note));
                if index + 1 < self.notes.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str("  ]\n");
        }
        out.push_str("}\n");
        out
    }

    pub fn parse(text: &str) -> Result<Manifest, String> {
        let value = Json::parse(text)?;
        let object = value.as_object()?;
        let splits = field(object, "splits")?.as_object()?;
        let test = field(splits, "test")?.as_object()?;
        let tuning = field(splits, "tuning")?.as_object()?;
        let seeds = field(object, "seeds")?.as_object()?;
        let notes = field(object, "notes")?.as_array()?;
        let mut notes_out = Vec::with_capacity(notes.len());
        for note in notes {
            notes_out.push(note.as_string()?.to_string());
        }
        Ok(Manifest {
            format: as_u32(field(object, "format")?, "format")?,
            goal: as_u32(field(object, "goal")?, "goal")?,
            pieces: as_u32(field(object, "pieces")?, "pieces")?,
            cap: as_u32(field(object, "cap")?, "cap")?,
            seeds: (
                as_u32(field(seeds, "first")?, "seeds.first")?,
                as_u32(field(seeds, "last")?, "seeds.last")?,
            ),
            test_split: (
                as_u32(field(test, "first")?, "splits.test.first")?,
                as_u32(field(test, "last")?, "splits.test.last")?,
            ),
            tuning_split: (
                as_u32(field(tuning, "first")?, "splits.tuning.first")?,
                as_u32(field(tuning, "last")?, "splits.tuning.last")?,
            ),
            game_count: as_u32(field(object, "game_count")?, "game_count")?,
            generator_version: as_u32(field(object, "generator_version")?, "generator_version")?,
            generator_description: field(object, "generator_description")?
                .as_string()?
                .to_string(),
            fingerprint: field(object, "fingerprint")?.as_string()?.to_string(),
            notes: notes_out,
        })
    }
}

fn field<'a>(object: &'a [(String, Json)], key: &str) -> Result<&'a Json, String> {
    object
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
        .ok_or_else(|| format!("manifest field `{key}` is missing"))
}

fn as_u32(value: &Json, key: &str) -> Result<u32, String> {
    match value {
        Json::Number(n) if *n >= 0 && *n <= u32::MAX as i64 => Ok(*n as u32),
        other => Err(format!(
            "manifest field `{key}` must be a u32, found {other:?}"
        )),
    }
}

fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
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

// A small JSON reader: the manifest is a dozen scalars, an array of strings and
// two nested objects. serde would be a dependency in a workspace that has none,
// and a line-based reader would break the moment someone reformats the file.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Json {
    Null,
    Bool(bool),
    Number(i64),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    fn parse(text: &str) -> Result<Json, String> {
        let bytes = text.as_bytes();
        let mut reader = Reader { bytes, at: 0 };
        let value = reader.value()?;
        reader.whitespace();
        if reader.at != bytes.len() {
            return Err(format!(
                "trailing bytes after the JSON value at byte {}",
                reader.at
            ));
        }
        Ok(value)
    }

    fn as_object(&self) -> Result<&[(String, Json)], String> {
        match self {
            Json::Object(entries) => Ok(entries),
            other => Err(format!("expected a JSON object, found {other:?}")),
        }
    }

    fn as_array(&self) -> Result<&[Json], String> {
        match self {
            Json::Array(items) => Ok(items),
            other => Err(format!("expected a JSON array, found {other:?}")),
        }
    }

    fn as_string(&self) -> Result<&str, String> {
        match self {
            Json::String(text) => Ok(text),
            other => Err(format!("expected a JSON string, found {other:?}")),
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn whitespace(&mut self) {
        while let Some(byte) = self.bytes.get(self.at) {
            if matches!(byte, b' ' | b'\t' | b'\r' | b'\n') {
                self.at += 1;
            } else {
                break;
            }
        }
    }

    fn eat(&mut self, expected: u8) -> Result<(), String> {
        self.whitespace();
        if self.bytes.get(self.at) == Some(&expected) {
            self.at += 1;
            Ok(())
        } else {
            Err(format!(
                "expected {:?} at byte {} of the manifest",
                expected as char, self.at
            ))
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.whitespace();
        match self.bytes.get(self.at).copied() {
            None => Err("the manifest is empty or truncated".into()),
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => Ok(Json::String(self.string()?)),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(byte) if byte == b'-' || byte.is_ascii_digit() => self.number(),
            Some(byte) => Err(format!(
                "unexpected byte {:?} at byte {} of the manifest",
                byte as char, self.at
            )),
        }
    }

    fn literal(&mut self, text: &str, value: Json) -> Result<Json, String> {
        if self.bytes[self.at..].starts_with(text.as_bytes()) {
            self.at += text.len();
            Ok(value)
        } else {
            Err(format!("expected `{text}` at byte {}", self.at))
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.eat(b'{')?;
        let mut entries = Vec::new();
        self.whitespace();
        if self.bytes.get(self.at) == Some(&b'}') {
            self.at += 1;
            return Ok(Json::Object(entries));
        }
        loop {
            let key = self.string()?;
            self.eat(b':')?;
            let value = self.value()?;
            entries.push((key, value));
            self.whitespace();
            match self.bytes.get(self.at) {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Json::Object(entries));
                }
                _ => return Err(format!("expected , or }} at byte {}", self.at)),
            }
        }
    }

    fn array(&mut self) -> Result<Json, String> {
        self.eat(b'[')?;
        let mut items = Vec::new();
        self.whitespace();
        if self.bytes.get(self.at) == Some(&b']') {
            self.at += 1;
            return Ok(Json::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.whitespace();
            match self.bytes.get(self.at) {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(format!("expected , or ] at byte {}", self.at)),
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.eat(b'"')?;
        let mut out = String::new();
        loop {
            let byte = self
                .bytes
                .get(self.at)
                .copied()
                .ok_or_else(|| "unterminated string in the manifest".to_string())?;
            self.at += 1;
            match byte {
                b'"' => return Ok(out),
                b'\\' => {
                    let escape = self
                        .bytes
                        .get(self.at)
                        .copied()
                        .ok_or_else(|| "unterminated escape in the manifest".to_string())?;
                    self.at += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hex = self
                                .bytes
                                .get(self.at..self.at + 4)
                                .ok_or_else(|| "truncated \\u escape".to_string())?;
                            let hex = std::str::from_utf8(hex)
                                .map_err(|_| "bad \\u escape".to_string())?;
                            let code = u32::from_str_radix(hex, 16)
                                .map_err(|_| format!("bad \\u escape: {hex:?}"))?;
                            self.at += 4;
                            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                        }
                        other => {
                            return Err(format!("unknown escape \\{}", other as char));
                        }
                    }
                }
                byte if byte < 0x80 => out.push(byte as char),
                _ => {
                    // Re-read the whole UTF-8 sequence.
                    let start = self.at - 1;
                    let width = utf8_width(byte);
                    let end = (start + width).min(self.bytes.len());
                    out.push_str(
                        std::str::from_utf8(&self.bytes[start..end])
                            .map_err(|_| "invalid UTF-8 in the manifest".to_string())?,
                    );
                    self.at = end;
                }
            }
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.at;
        if self.bytes.get(self.at) == Some(&b'-') {
            self.at += 1;
        }
        while let Some(byte) = self.bytes.get(self.at) {
            if byte.is_ascii_digit() {
                self.at += 1;
            } else {
                break;
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.at])
            .map_err(|_| "invalid UTF-8 in a number".to_string())?;
        text.parse::<i64>()
            .map(Json::Number)
            .map_err(|_| format!("the manifest's numbers must be integers, found {text:?}"))
    }
}

fn utf8_width(first: u8) -> usize {
    match first {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_is_the_spec_line() {
        assert_eq!(
            header_line(&GOAL8),
            "# tetris-lab dataset 1 goal=8 pieces=40 cap=7 seeds=0-1999"
        );
        assert_eq!(
            header_line(&GOAL12),
            "# tetris-lab dataset 1 goal=12 pieces=100 cap=7 seeds=0-1999"
        );
    }

    #[test]
    fn a_game_line_round_trips() {
        let game = GameLine {
            seed: 512,
            board: [0, 9, 0xd, 7, 3, 2, 0x1a, 0x3f, 0x3c, 0],
            sequence: b"JIOZSLTJZOITLSZTSLJOILZIOTJSTJOLISZSZLOI".to_vec(),
        };
        let line = game_line(&game);
        assert!(line.starts_with("512 00000000 00000009 0000000d 00000007 "));
        assert!(line.ends_with(" JIOZSLTJZOITLSZTSLJOILZIOTJSTJOLISZSZLOI"));
        let file = format!("{}\n{}\n", header_line(&GOAL8), line);
        let parsed = parse(&GOAL8, &file).unwrap();
        assert_eq!(parsed.games, vec![game]);
        assert_eq!(parsed.header, Header::new(&GOAL8));
    }

    #[test]
    fn parsing_is_semantic() {
        let letters: Vec<u8> = b"IJLOSTZ"
            .iter()
            .copied()
            .cycle()
            .take(GOAL8.pieces as usize)
            .collect();
        let game = GameLine {
            seed: 0,
            board: [0; 10],
            sequence: letters.clone(),
        };
        let file = format!("{}\n{}\n", header_line(&GOAL8), game_line(&game));
        // The same game, different bytes: CRLF, three-space separators, a
        // trailing space, and the hex in uppercase.
        let words = "00000000  ".repeat(9) + "00000000 ";
        let reformatted = format!(
            "{}\r\n0  {} {} \r\n",
            header_line(&GOAL8),
            words.to_uppercase(),
            String::from_utf8(letters).unwrap()
        );
        assert_ne!(file.len(), reformatted.len());
        assert_eq!(
            parse(&GOAL8, &file).unwrap().games,
            parse(&GOAL8, &reformatted).unwrap().games
        );
    }

    #[test]
    fn a_short_sequence_is_a_named_error() {
        let file = format!(
            "{}\n0 {} IJLOSTZ\n",
            header_line(&GOAL8),
            "00000000 ".repeat(10).trim_end()
        );
        let error = parse(&GOAL8, &file).unwrap_err();
        assert_eq!(error.line, 2);
        assert!(error.message.contains("7 letters"), "{}", error.message);
    }

    #[test]
    fn the_manifest_round_trips() {
        let mut manifest = Manifest::for_config(&GOAL8, "4bf1a17b1685a774");
        manifest
            .notes
            .push("a reason with \"quotes\" and \\ a backslash".into());
        manifest.notes.push("a second line".into());
        manifest.generator_version = 2;
        let text = manifest.render();
        assert_eq!(Manifest::parse(&text).unwrap(), manifest);
    }
}
