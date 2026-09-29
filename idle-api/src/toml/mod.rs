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

mod parser;
mod parser_str;
mod parser_value;
use parser::Parser;

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
mod tests;
