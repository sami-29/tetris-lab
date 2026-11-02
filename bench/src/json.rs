//! A JSON reader, in this crate's own vocabulary.
//!
//! `bench` has no dependencies by design (`run/log.md`), and one of the artifacts
//! it writes has to be read back: `bench width-curve --summarise <path>` recomputes
//! the width curve's *derived* summary fields from the evidence already committed,
//! without re-measuring, so those numbers have to come out of the file again. Every
//! other reader in this crate is a single-field scanner (`width::json_i64`), which
//! is enough for one integer and not enough for a series of points.
//!
//! This module is exactly the subset `width::Artifact::render` emits — objects,
//! arrays, strings, numbers, `true`/`false`/`null` — and nothing else: no comments,
//! no trailing commas, no bare keys. It is pinned by round-tripping the committed
//! artifact (`bench/tests/width.rs`): parse-then-render has to reproduce the file
//! byte for byte, or a reader that quietly loses precision would go unnoticed until
//! it rewrote a measured number.
//!
//! The one representational decision: every number is an `f64`. The artifact's
//! floats are printed with two or three decimals and its integers are counts well
//! under 2^53, so `f64` is exact for both, and giving the reader a single numeric
//! type keeps it small. Callers that need an integer say so (`Value::as_u32`), and
//! a fractional value is an error there rather than a silent truncation.

/// A JSON value, in document order. Objects keep their fields as a list rather
/// than a map so a round trip cannot reorder an artifact's keys.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    /// One field of an object, or `None` for a miss *and* for a non-object.
    /// Callers that need to tell those apart name the field in the error.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(fields) => fields
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(number) => Some(*number),
            _ => None,
        }
    }

    /// A non-negative integer that fits a `u32`. A count is never fractional, and
    /// a reader that rounded one would be inventing evidence.
    pub fn as_u32(&self) -> Option<u32> {
        let number = self.as_f64()?;
        if number.fract() != 0.0 || number < 0.0 || number > u32::MAX as f64 {
            return None;
        }
        Some(number as u32)
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&[(String, Value)]> {
        match self {
            Value::Object(fields) => Some(fields),
            _ => None,
        }
    }

    /// The JSON type, for error messages: "expected a number, found an array".
    pub fn kind(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "a boolean",
            Value::Number(_) => "a number",
            Value::String(_) => "a string",
            Value::Array(_) => "an array",
            Value::Object(_) => "an object",
        }
    }
}

/// Parse one JSON document. Trailing whitespace is allowed; trailing content is
/// not, so a truncated file cannot be read as a shorter valid one.
pub fn parse(text: &str) -> Result<Value, String> {
    let mut parser = Parser {
        bytes: text.as_bytes(),
        at: 0,
    };
    let value = parser.value()?;
    parser.space();
    if parser.at != parser.bytes.len() {
        return Err(parser.error("trailing content after the document"));
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Parser<'a> {
    fn error(&self, what: &str) -> String {
        format!("{what} (at byte {})", self.at)
    }

    fn space(&mut self) {
        while matches!(self.bytes.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn want(&mut self, byte: u8) -> Result<(), String> {
        if self.peek() != Some(byte) {
            let found = match self.peek() {
                Some(byte) => format!("found {:?}", byte as char),
                None => "found the end of the file".to_string(),
            };
            return Err(self.error(&format!("expected {:?}, {found}", byte as char)));
        }
        self.at += 1;
        Ok(())
    }

    fn value(&mut self) -> Result<Value, String> {
        self.space();
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Value::String),
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            Some(b'n') => self.literal("null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(byte) => Err(self.error(&format!("unexpected {:?}", byte as char))),
            None => Err(self.error("unexpected end of the file")),
        }
    }

    fn literal(&mut self, word: &str, value: Value) -> Result<Value, String> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Ok(value)
        } else {
            Err(self.error(&format!("expected {word}")))
        }
    }

    fn object(&mut self) -> Result<Value, String> {
        self.want(b'{')?;
        let mut fields = Vec::new();
        self.space();
        if self.peek() == Some(b'}') {
            self.at += 1;
            return Ok(Value::Object(fields));
        }
        loop {
            self.space();
            let key = self.string()?;
            self.space();
            self.want(b':')?;
            let value = self.value()?;
            fields.push((key, value));
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Value::Object(fields));
                }
                _ => return Err(self.error("expected ',' or '}' in an object")),
            }
        }
    }

    fn array(&mut self) -> Result<Value, String> {
        self.want(b'[')?;
        let mut items = Vec::new();
        self.space();
        if self.peek() == Some(b']') {
            self.at += 1;
            return Ok(Value::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.space();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {
                    self.at += 1;
                    return Ok(Value::Array(items));
                }
                _ => return Err(self.error("expected ',' or ']' in an array")),
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.want(b'"')?;
        let mut out = String::new();
        loop {
            let byte = self
                .peek()
                .ok_or_else(|| self.error("unterminated string"))?;
            self.at += 1;
            match byte {
                b'"' => return Ok(out),
                b'\\' => {
                    let escape = self
                        .peek()
                        .ok_or_else(|| self.error("unterminated escape"))?;
                    self.at += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{0008}'),
                        b'f' => out.push('\u{000c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.unicode_escape()?),
                        other => {
                            return Err(self.error(&format!("unknown escape \\{}", other as char)))
                        }
                    }
                }
                byte => {
                    // The artifact is UTF-8 and the writer emits it verbatim, so a
                    // multi-byte character has to be gathered whole: `render` writes
                    // `≈` and `§` into prose fields.
                    let start = self.at - 1;
                    let width = utf8_width(byte);
                    if width == 0 {
                        return Err(self.error("a string byte that is not UTF-8"));
                    }
                    while self.at < start + width {
                        match self.peek() {
                            Some(continuation) if continuation & 0xC0 == 0x80 => self.at += 1,
                            _ => return Err(self.error("a truncated UTF-8 sequence")),
                        }
                    }
                    match std::str::from_utf8(&self.bytes[start..start + width]) {
                        Ok(text) => out.push_str(text),
                        Err(_) => return Err(self.error("a string byte that is not UTF-8")),
                    }
                }
            }
        }
    }

    /// `\uXXXX`, with the surrogate pair the writer never emits but a reader
    /// should still understand (a lone surrogate is not a character).
    fn unicode_escape(&mut self) -> Result<char, String> {
        let mut unit = 0u32;
        for _ in 0..4 {
            let byte = self
                .peek()
                .ok_or_else(|| self.error("truncated \\u escape"))?;
            let digit = match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => return Err(self.error("a \\u escape that is not hexadecimal")),
            };
            unit = unit * 16 + digit as u32;
            self.at += 1;
        }
        if (0xD800..0xDC00).contains(&unit) {
            // High surrogate: the low half must follow as `\uXXXX`.
            self.want(b'\\')?;
            self.want(b'u')?;
            let mut low = 0u32;
            for _ in 0..4 {
                let byte = self
                    .peek()
                    .ok_or_else(|| self.error("truncated \\u escape"))?;
                let digit = match byte {
                    b'0'..=b'9' => byte - b'0',
                    b'a'..=b'f' => byte - b'a' + 10,
                    b'A'..=b'F' => byte - b'A' + 10,
                    _ => return Err(self.error("a \\u escape that is not hexadecimal")),
                };
                low = low * 16 + digit as u32;
                self.at += 1;
            }
            if !(0xDC00..0xE000).contains(&low) {
                return Err(self.error("a high surrogate with no low surrogate"));
            }
            let code = 0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00);
            return char::from_u32(code).ok_or_else(|| self.error("a surrogate pair out of range"));
        }
        char::from_u32(unit).ok_or_else(|| self.error("a lone surrogate"))
    }

    fn number(&mut self) -> Result<Value, String> {
        let start = self.at;
        if self.peek() == Some(b'-') {
            self.at += 1;
        }
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.at += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.at += 1;
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.at])
            .map_err(|_| self.error("a number that is not UTF-8"))?;
        text.parse::<f64>()
            .map(Value::Number)
            .map_err(|_| self.error(&format!("{text:?} is not a number")))
    }
}

/// The length in bytes of the UTF-8 sequence a leading byte announces, or 0 for a
/// byte that cannot lead one.
fn utf8_width(byte: u8) -> usize {
    match byte {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reader_handles_the_subset_the_writer_emits() {
        let value = parse(
            r#"{
              "a": 1, "b": -2.5, "c": "x\"y",
              "d": [true, false, null],
              "e": { "f": 1e3 },
              "g": "≈ 215 ms (§9.5)"
            }"#,
        )
        .expect("parse");
        assert_eq!(value.get("a").and_then(Value::as_u32), Some(1));
        assert_eq!(value.get("b").and_then(Value::as_f64), Some(-2.5));
        assert_eq!(value.get("c").and_then(Value::as_str), Some("x\"y"));
        assert_eq!(
            value
                .get("d")
                .and_then(Value::as_array)
                .map(|items| items.len()),
            Some(3)
        );
        assert_eq!(
            value
                .get("e")
                .and_then(|inner| inner.get("f"))
                .and_then(Value::as_f64),
            Some(1_000.0)
        );
        assert_eq!(
            value.get("g").and_then(Value::as_str),
            Some("≈ 215 ms (§9.5)")
        );
    }

    #[test]
    fn the_reader_refuses_what_the_writer_never_would_have_written() {
        // A truncated document is not a shorter valid one: the points array is
        // where that would be most dangerous.
        assert!(parse(r#"{"points": [1, 2"#).is_err());
        assert!(parse(r#"{"a": 1} {"b": 2}"#).is_err());
        assert!(parse("{a: 1}").is_err());
        assert!(parse(r#"{"a": 1,}"#).is_err());
        assert!(parse("").is_err());
    }

    #[test]
    fn a_count_is_an_integer_or_it_is_nothing() {
        assert_eq!(Value::Number(600.0).as_u32(), Some(600));
        assert_eq!(Value::Number(600.5).as_u32(), None);
        assert_eq!(Value::Number(-1.0).as_u32(), None);
        assert_eq!(Value::Number(1e20).as_u32(), None);
        assert_eq!(Value::String("600".to_string()).as_u32(), None);
    }

    #[test]
    fn escapes_and_unicode_come_back_whole() {
        let value = parse(r#""a\u00e9\u20ac\ud83d\ude00\n""#).expect("parse");
        assert_eq!(value.as_str(), Some("aé€😀\n"));
        assert!(
            parse(r#""\ud83d""#).is_err(),
            "a lone surrogate is not a character"
        );
    }
}
