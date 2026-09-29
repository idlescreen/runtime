// SPDX-License-Identifier: Apache-2.0

use super::parser::Parser;
use super::parser_value::utf8_len;
use super::*;

impl<'a> Parser<'a> {
    pub(crate) fn string(&mut self) -> Result<String, Error> {
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

    pub(crate) fn literal_string(&mut self) -> Result<String, Error> {
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

    pub(crate) fn hex(&mut self, n: usize) -> Result<u32, Error> {
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
}
