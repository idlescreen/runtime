// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) struct Parser<'a> {
    pub(crate) bytes: &'a [u8],
    pub(crate) pos: usize,
    pub(crate) line: usize,
    pub(crate) col: usize,
}

impl<'a> Parser<'a> {
    pub(crate) fn err(&self, msg: impl Into<String>) -> Error {
        Error::new(msg, self.line, self.col)
    }

    pub(crate) fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    pub(crate) fn bump(&mut self) -> Option<u8> {
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
    pub(crate) fn skip_inline_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.bump();
        }
    }

    /// Skip whitespace including newlines and `#` comments.
    pub(crate) fn skip_ws_comments(&mut self) {
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
    pub(crate) fn expect_line_end(&mut self) -> Result<(), Error> {
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

    pub(crate) fn document(&mut self) -> Result<Value, Error> {
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

    pub(crate) fn key(&mut self) -> Result<String, Error> {
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

    pub(crate) fn value(&mut self) -> Result<Value, Error> {
        match self.peek() {
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b'\'') => Ok(Value::Str(self.literal_string()?)),
            Some(b'[') => self.array(),
            Some(b't' | b'f') => self.boolean(),
            Some(b'-' | b'+' | b'0'..=b'9') => self.integer(),
            _ => Err(self.err("expected value")),
        }
    }
}
