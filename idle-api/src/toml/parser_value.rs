// SPDX-License-Identifier: Apache-2.0

use super::parser::Parser;
use super::*;

impl<'a> Parser<'a> {
    pub(crate) fn array(&mut self) -> Result<Value, Error> {
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

    pub(crate) fn boolean(&mut self) -> Result<Value, Error> {
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

    pub(crate) fn integer(&mut self) -> Result<Value, Error> {
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

pub(crate) fn utf8_len(first: u8) -> usize {
    match first {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}
