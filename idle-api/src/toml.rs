// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Minimal TOML-subset parser for `.idleplugin.toml` manifests.
//!
//! Supports exactly the schema surface: `key = value` pairs, `[table]`
//! headers, `#` comments, basic strings (with `\"`/`\\`/`\'"`-free
//! escapes), integers, booleans, and arrays (possibly multiline) of
//! strings/ints/bools. Dotted keys, inline tables, floats, and dates are
//! rejected — none appear in schema v1 manifests.

use std::fmt;

/// A parsed TOML value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Int(i64),
    Bool(bool),
    Array(Vec<Value>),
    Table(Vec<(String, Value)>),
}

/// Parse error with 1-based line/column.
#[derive(Debug)]
pub struct Error {
    msg: String,
    line: usize,
    col: usize,
}

impl Error {
    fn new(msg: impl Into<String>, line: usize, col: usize) -> Self {
        Self {
            msg: msg.into(),
            line,
            col,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} (at line {}, column {})",
            self.msg, self.line, self.col
        )
    }
}

impl std::error::Error for Error {}

/// Parse a TOML document into a table of top-level key/value pairs.
pub fn parse(text: &str) -> Result<Value, Error> {
    let mut p = Parser {
        bytes: text.as_bytes(),
        pos: 0,
        line: 1,
        col: 1,
    };
    p.document()
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    line: usize,
    col: usize,
}

impl<'a> Parser<'a> {
    fn err(&self, msg: impl Into<String>) -> Error {
        Error::new(msg, self.line, self.col)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.pos += 1;
        if b == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(b)
    }

    /// Skip spaces/tabs (not newlines).
    fn skip_inline_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.bump();
        }
    }

    /// Skip whitespace including newlines and `#` comments.
    fn skip_ws_comments(&mut self) {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\n' | b'\r') => {
                    self.bump();
                }
                Some(b'#') => {
                    while !matches!(self.peek(), None | Some(b'\n')) {
                        self.bump();
                    }
                }
                _ => return,
            }
        }
    }

    /// Skip rest of line: optional comment then newline-or-EOF.
    fn expect_line_end(&mut self) -> Result<(), Error> {
        self.skip_inline_ws();
        match self.peek() {
            Some(b'#') => {
                while !matches!(self.peek(), None | Some(b'\n')) {
                    self.bump();
                }
            }
            Some(b'\r') => {
                self.bump();
                if self.peek() != Some(b'\n') {
                    return Err(self.err("expected newline"));
                }
            }
            Some(b'\n') | None => {}
            Some(_) => return Err(self.err("expected comment or newline")),
        }
        if self.peek() == Some(b'\n') {
            self.bump();
        }
        Ok(())
    }

    fn document(&mut self) -> Result<Value, Error> {
        // Top-level table plus named sub-tables, returned as one nested Value.
        let mut root: Vec<(String, Value)> = Vec::new();
        let mut current: Option<String> = None;
        loop {
            self.skip_ws_comments();
            match self.peek() {
                None => break,
                Some(b'[') => {
                    self.bump();
                    self.skip_inline_ws();
                    let name = self.key()?;
                    self.skip_inline_ws();
                    if self.bump() != Some(b']') {
                        return Err(self.err("expected ']'"));
                    }
                    self.expect_line_end()?;
                    if root.iter().any(|(k, _)| *k == name) {
                        return Err(self.err(format!("duplicate table [{name}]")));
                    }
                    root.push((name.clone(), Value::Table(Vec::new())));
                    current = Some(name);
                }
                Some(_) => {
                    let key = self.key()?;
                    self.skip_inline_ws();
                    if self.bump() != Some(b'=') {
                        return Err(self.err("expected '='"));
                    }
                    self.skip_inline_ws();
                    let v = self.value()?;
                    self.expect_line_end()?;
                    let table = match &current {
                        None => &mut root,
                        Some(name) => match root.iter_mut().find(|(k, _)| k == name) {
                            Some((_, Value::Table(t))) => t,
                            _ => unreachable!(),
                        },
                    };
                    if table.iter().any(|(k, _)| *k == key) {
                        return Err(self.err(format!("duplicate key '{key}'")));
                    }
                    table.push((key, v));
                }
            }
        }
        Ok(Value::Table(root))
    }

    fn key(&mut self) -> Result<String, Error> {
        let start = self.pos;
        match self.peek() {
            Some(b'"') => return self.string(),
            Some(c) if c.is_ascii_alphanumeric() || c == b'_' || c == b'-' => {
                while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
                {
                    self.bump();
                }
                // `a.b = 1` is a nested-table key in real TOML — outside this
                // subset, and storing it flat would silently misparse.
                if self.peek() == Some(b'.') {
                    return Err(self.err("dotted keys are not supported"));
                }
            }
            _ => return Err(self.err("expected key")),
        }
        Ok(std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| self.err("invalid key"))?
            .to_string())
    }

    fn value(&mut self) -> Result<Value, Error> {
        match self.peek() {
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b'\'') => Ok(Value::Str(self.literal_string()?)),
            Some(b'[') => self.array(),
            Some(b't' | b'f') => self.boolean(),
            Some(b'-' | b'+' | b'0'..=b'9') => self.integer(),
            _ => Err(self.err("expected value")),
        }
    }

    fn string(&mut self) -> Result<String, Error> {
        debug_assert_eq!(self.peek(), Some(b'"'));
        self.bump();
        let mut out = String::new();
        loop {
            let b = self.bump().ok_or_else(|| self.err("unterminated string"))?;
            match b {
                b'"' => return Ok(out),
                b'\\' => match self.bump() {
                    Some(b'"') => out.push('"'),
                    Some(b'\\') => out.push('\\'),
                    Some(b'n') => out.push('\n'),
                    Some(b't') => out.push('\t'),
                    Some(b'r') => out.push('\r'),
                    Some(b'b') => out.push('\u{0008}'),
                    Some(b'f') => out.push('\u{000C}'),
                    Some(b'u') => {
                        let cp = self.hex(4)?;
                        out.push(char::from_u32(cp).ok_or_else(|| self.err("invalid \\u escape"))?);
                    }
                    Some(b'U') => {
                        let cp = self.hex(8)?;
                        out.push(char::from_u32(cp).ok_or_else(|| self.err("invalid \\U escape"))?);
                    }
                    _ => return Err(self.err("invalid escape")),
                },
                0x00..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F | 0x7F => {
                    return Err(self.err("control character in string"));
                }
                _ => {
                    let start = self.pos - 1;
                    let len = utf8_len(b);
                    self.pos += len - 1;
                    self.col += len - 1;
                    out.push_str(
                        std::str::from_utf8(&self.bytes[start..self.pos])
                            .map_err(|_| self.err("invalid utf-8"))?,
                    );
                }
            }
        }
    }

    fn literal_string(&mut self) -> Result<String, Error> {
        debug_assert_eq!(self.peek(), Some(b'\''));
        self.bump();
        let mut out = String::new();
        loop {
            let b = self.bump().ok_or_else(|| self.err("unterminated string"))?;
            if b == b'\'' {
                return Ok(out);
            }
            let start = self.pos - 1;
            let len = utf8_len(b);
            self.pos += len - 1;
            self.col += len - 1;
            out.push_str(
                std::str::from_utf8(&self.bytes[start..self.pos])
                    .map_err(|_| self.err("invalid utf-8"))?,
            );
        }
    }

    fn hex(&mut self, n: usize) -> Result<u32, Error> {
        let mut v = 0u32;
        for _ in 0..n {
            let d = match self.bump() {
                Some(c @ b'0'..=b'9') => c - b'0',
                Some(c @ b'a'..=b'f') => c - b'a' + 10,
                Some(c @ b'A'..=b'F') => c - b'A' + 10,
                _ => return Err(self.err("invalid unicode escape")),
            };
            v = v * 16 + d as u32;
        }
        Ok(v)
    }

    fn array(&mut self) -> Result<Value, Error> {
        debug_assert_eq!(self.peek(), Some(b'['));
        self.bump();
        let mut items = Vec::new();
        loop {
            self.skip_ws_comments();
            match self.peek() {
                Some(b']') => {
                    self.bump();
                    return Ok(Value::Array(items));
                }
                Some(_) => {
                    items.push(self.value()?);
                    self.skip_ws_comments();
                    match self.bump() {
                        Some(b',') => continue,
                        Some(b']') => return Ok(Value::Array(items)),
                        _ => return Err(self.err("expected ',' or ']'")),
                    }
                }
                None => return Err(self.err("unterminated array")),
            }
        }
    }

    fn boolean(&mut self) -> Result<Value, Error> {
        if self.bytes[self.pos..].starts_with(b"true") {
            for _ in 0..4 {
                self.bump();
            }
            Ok(Value::Bool(true))
        } else if self.bytes[self.pos..].starts_with(b"false") {
            for _ in 0..5 {
                self.bump();
            }
            Ok(Value::Bool(false))
        } else {
            Err(self.err("invalid boolean"))
        }
    }

    fn integer(&mut self) -> Result<Value, Error> {
        let start = self.pos;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.bump();
        }
        if !matches!(self.peek(), Some(b'0'..=b'9')) {
            return Err(self.err("invalid integer"));
        }
        while matches!(self.peek(), Some(b'0'..=b'9' | b'_')) {
            self.bump();
        }
        let text: String = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| self.err("invalid integer"))?
            .chars()
            .filter(|&c| c != '_')
            .collect();
        text.parse::<i64>()
            .map(Value::Int)
            .map_err(|_| self.err("integer out of range"))
    }
}

fn utf8_len(first: u8) -> usize {
    match first {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

impl Value {
    /// `(key, value)` pairs of a table value.
    pub fn as_table(&self) -> Option<&[(String, Value)]> {
        match self {
            Value::Table(t) => Some(t),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_table()
            .and_then(|t| t.iter().find(|(k, _)| k == key).map(|(_, v)| v))
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_manifest_shape() {
        let text = r#"
# comment
schema_version = 1
plugin_id      = "io.idlescreen.saver.Aurora"
plugin_version = "2.4.0"
api_version    = 1

[entry]
runtime = "native"
library = "libscreensaver_aurora.so"

[capabilities]
network          = false
filesystem_read  = []
filesystem_write = ["/tmp/x", "/opt/y"]

[dependencies]
native = ["libc6"]
wasm   = []

[headless_render]
default_fps        = 60
deterministic_seed = true
gpu_optional       = true
"#;
        let doc = parse(text).unwrap();
        assert_eq!(doc.get("schema_version").unwrap().as_int(), Some(1));
        let entry = doc.get("entry").unwrap();
        assert_eq!(entry.get("runtime").unwrap().as_str(), Some("native"));
        let caps = doc.get("capabilities").unwrap();
        assert_eq!(caps.get("network").unwrap().as_bool(), Some(false));
        let fw = caps.get("filesystem_write").unwrap().as_array().unwrap();
        assert_eq!(fw.len(), 2);
        assert_eq!(fw[0].as_str(), Some("/tmp/x"));
    }

    #[test]
    fn rejects_junk() {
        assert!(parse("key = ").is_err());
        assert!(parse("[unclosed").is_err());
        assert!(parse("a = 1\na = 2").is_err());
    }

    #[test]
    fn string_escapes_decode() {
        let doc = parse("a = \"x\\ny\\t\\\"z\\\" \\\\ \\u0041\"").unwrap();
        assert_eq!(doc.get("a").unwrap().as_str(), Some("x\ny\t\"z\" \\ A"));
    }

    #[test]
    fn string_rejects_bad_escape_and_control_chars() {
        assert!(parse("a = \"x\\q\"").is_err(), "invalid escape");
        assert!(parse("a = \"x\\uD800\"").is_err(), "surrogate codepoint");
        assert!(parse("a = \"x\u{0001}y\"").is_err(), "control char");
        assert!(parse("a = \"unterminated").is_err());
    }

    #[test]
    fn literal_strings_take_chars_verbatim() {
        let doc = parse("a = 'C:\\new\\path'").unwrap();
        assert_eq!(doc.get("a").unwrap().as_str(), Some("C:\\new\\path"));
        assert!(parse("a = 'unterminated").is_err());
    }

    #[test]
    fn arrays_multiline_comments_and_trailing_comma() {
        let doc = parse("a = [\n  \"x\", # first\n  \"y\",\n]\nb = [1, 2,]\nc = []\n").unwrap();
        let a = doc.get("a").unwrap().as_array().unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!(a[1].as_str(), Some("y"));
        assert_eq!(doc.get("b").unwrap().as_array().unwrap().len(), 2);
        assert_eq!(doc.get("c").unwrap().as_array().unwrap().len(), 0);
    }

    #[test]
    fn array_missing_comma_is_error() {
        assert!(parse("a = [\"x\" \"y\"]").is_err());
        assert!(parse("a = [\"x\"").is_err(), "unterminated");
    }

    #[test]
    fn integers_signs_underscores_and_bounds() {
        let doc = parse("a = +42\nb = -7\nc = 1_000_000\nd = 0").unwrap();
        assert_eq!(doc.get("a").unwrap().as_int(), Some(42));
        assert_eq!(doc.get("b").unwrap().as_int(), Some(-7));
        assert_eq!(doc.get("c").unwrap().as_int(), Some(1_000_000));
        assert!(parse("a = 99999999999999999999").is_err(), "i64 overflow");
        assert!(parse("a = 0x10").is_err(), "hex rejected");
        assert!(parse("a = 1.5").is_err(), "floats rejected");
    }

    #[test]
    fn tables_can_interleave_root_keys() {
        // Root keys after a [table] belong to the table, not root.
        let doc = parse("root1 = 1\n[t]\nk = 2\nroot2 = 3\n").unwrap();
        assert_eq!(doc.get("root1").unwrap().as_int(), Some(1));
        assert!(doc.get("root2").is_none(), "goes to [t], not root");
        assert_eq!(
            doc.get("t").unwrap().get("root2").unwrap().as_int(),
            Some(3)
        );
    }

    #[test]
    fn duplicate_table_and_key_rejected() {
        assert!(parse("[t]\na=1\n[t]\nb=2").is_err(), "dup table");
        assert!(parse("[t]\na=1\na=2").is_err(), "dup key in table");
        let err = parse("a = 1\n[t]\nb = 2\n[t]\nc = 3").unwrap_err();
        assert!(format!("{err}").contains("duplicate table"));
    }

    #[test]
    fn dotted_keys_rejected() {
        // `a.b` is a nested-table key in real TOML — outside this subset,
        // so it's an error rather than silently stored as a flat name.
        let err = parse("a.b = 1").unwrap_err();
        assert!(format!("{err}").contains("dotted"), "{err}");
        assert!(parse("a.b.c = 1").is_err());
    }

    #[test]
    fn malformed_inputs_never_panic() {
        // Every prefix of a valid manifest — Err or Ok, never panic.
        let good = "[entry]\nlibrary = \"x.so\"\n[caps]\nnet = false\nlist = [\"a\", \"b\"]\nn = -4_2\n";
        for i in 0..=good.len() {
            let _ = parse(&good[..i]);
        }
        for bad in [
            "", "[", "]", "[a", "a", "=", "a =", "a = [", "a = [1", "\"", "'",
            "a = \"\\", "a = \"\\u", "a = \"\\uZZZZ\"", "[[]]", "[a]", "[[a]]",
            "a = {", "a = 0x", "a = 18446744073709551616", "a = +", "a = -",
            "\u{feff}a = 1", "a = '''x", "a = t", "a = truex", "a = \"x\"\ny",
            ".a = 1", "a. = 1", "- = 1", "_ = 1",
        ] {
            let _ = parse(bad);
        }
    }

    #[test]
    fn inline_tables_and_dates_rejected() {
        assert!(parse("a = {x = 1}").is_err(), "inline table");
        assert!(parse("a = 2024-01-01").is_err(), "date");
    }

    #[test]
    fn error_reports_line_and_column() {
        let err = parse("a = 1\nbad!! = 2").unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("line 2"), "got: {msg}");
        let err2 = parse("ok = 1\n\n\nx = {").unwrap_err();
        assert!(format!("{err2}").contains("line 4"));
    }

    #[test]
    fn crlf_line_endings_accepted() {
        let doc = parse("a = 1\r\nb = \"x\"\r\n").unwrap();
        assert_eq!(doc.get("a").unwrap().as_int(), Some(1));
    }

    #[test]
    fn value_accessors_return_none_on_wrong_type() {
        let doc = parse("s = \"x\"\ni = 1\nb = true\na = [1]").unwrap();
        assert!(doc.get("s").unwrap().as_int().is_none());
        assert!(doc.get("i").unwrap().as_str().is_none());
        assert!(doc.get("b").unwrap().as_array().is_none());
        assert!(doc.get("a").unwrap().as_bool().is_none());
        assert!(doc.get("missing").is_none());
    }

    #[test]
    fn comment_only_and_empty_documents() {
        assert_eq!(parse("").unwrap().as_table().unwrap().len(), 0);
        assert_eq!(
            parse("# just a comment\n\n# another\n")
                .unwrap()
                .as_table()
                .unwrap()
                .len(),
            0
        );
    }
}
