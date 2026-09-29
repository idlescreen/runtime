// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Minimal error plumbing replacing `anyhow`: a boxed error with context
//! chaining, `Result`, `Context`, `bail!`, `anyhow!`, `ensure!`,
//! `downcast_ref`, and anyhow-style `{:#}` ("ctx: cause: root") display.

use std::error::Error as StdError;
use std::fmt;

/// Boxed error chain. `context()` wraps the current error with a message.
pub struct Error {
    inner: Box<dyn StdError + Send + Sync>,
}

/// Leaf error carrying a plain message.
struct Msg(String);

impl fmt::Display for Msg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl fmt::Debug for Msg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl StdError for Msg {}

/// Context layer: prints as `msg` and exposes the wrapped error as source.
struct Ctx {
    msg: String,
    source: Box<dyn StdError + Send + Sync>,
}

impl fmt::Display for Ctx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg)
    }
}
impl fmt::Debug for Ctx {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.msg, self.source)
    }
}
impl StdError for Ctx {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&*self.source)
    }
}

impl Error {
    /// Error from a plain message (no source).
    pub fn msg(s: impl Into<String>) -> Self {
        Self {
            inner: Box::new(Msg(s.into())),
        }
    }

    /// Wrap any std error (`idle_err::Error::new`).
    pub fn new<E: StdError + Send + Sync + 'static>(e: E) -> Self {
        Self::from(e)
    }

    /// Wrap this error with a context message (`idle_err::Error::context`).
    pub fn context(self, msg: impl Into<String>) -> Self {
        Self {
            inner: Box::new(Ctx {
                msg: msg.into(),
                source: self.inner,
            }),
        }
    }

    /// First `T` in the chain, if any (matches `idle_err::Error::downcast_ref`).
    pub fn downcast_ref<T: StdError + 'static>(&self) -> Option<&T> {
        let mut cur: &(dyn StdError + 'static) = &*self.inner;
        loop {
            if let Some(t) = cur.downcast_ref::<T>() {
                return Some(t);
            }
            cur = cur.source()?;
        }
    }
}

impl<E> From<E> for Error
where
    E: StdError + Send + Sync + 'static,
{
    fn from(e: E) -> Self {
        Self { inner: Box::new(e) }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            // anyhow `{:#}`: "context: cause: root" on one line.
            let mut first = true;
            let mut cur: &(dyn StdError + 'static) = &*self.inner;
            loop {
                if !first {
                    write!(f, ": ")?;
                }
                first = false;
                write!(f, "{cur}")?;
                match cur.source() {
                    Some(s) => cur = s,
                    None => return Ok(()),
                }
            }
        }
        write!(f, "{}", self.inner)
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// `idle_err::Result<T>` equivalent.
pub type Result<T> = std::result::Result<T, Error>;

/// Attach context to a `Result` or `Option` (anyhow's `Context` trait).
pub trait Context<T> {
    fn context(self, msg: impl Into<String>) -> Result<T>;
    fn with_context(self, f: impl FnOnce() -> String) -> Result<T>;
}

impl<T, E> Context<T> for std::result::Result<T, E>
where
    E: StdError + Send + Sync + 'static,
{
    fn context(self, msg: impl Into<String>) -> Result<T> {
        self.map_err(|e| Error {
            inner: Box::new(Ctx {
                msg: msg.into(),
                source: Box::new(e),
            }),
        })
    }

    fn with_context(self, f: impl FnOnce() -> String) -> Result<T> {
        self.map_err(|e| Error {
            inner: Box::new(Ctx {
                msg: f(),
                source: Box::new(e),
            }),
        })
    }
}

impl<T> Context<T> for Result<T> {
    fn context(self, msg: impl Into<String>) -> Result<T> {
        self.map_err(|e| e.context(msg))
    }

    fn with_context(self, f: impl FnOnce() -> String) -> Result<T> {
        self.map_err(|e| e.context(f()))
    }
}

impl<T> Context<T> for Option<T> {
    fn context(self, msg: impl Into<String>) -> Result<T> {
        self.ok_or_else(|| Error::msg(msg.into()))
    }

    fn with_context(self, f: impl FnOnce() -> String) -> Result<T> {
        self.ok_or_else(|| Error::msg(f()))
    }
}

#[macro_export]
macro_rules! anyhow {
    ($($arg:tt)*) => {
        $crate::Error::msg(format!($($arg)*))
    };
}

#[macro_export]
macro_rules! bail {
    ($($arg:tt)*) => {
        return Err($crate::anyhow!($($arg)*))
    };
}

#[macro_export]
macro_rules! ensure {
    ($cond:expr $(,)?) => {
        if !($cond) {
            return Err($crate::Error::msg(concat!(
                "condition failed: `",
                stringify!($cond),
                "`"
            )));
        }
    };
    ($cond:expr, $($arg:tt)*) => {
        if !($cond) {
            return Err($crate::anyhow!($($arg)*));
        }
    };
}

#[cfg(test)]
mod tests;
