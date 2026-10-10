// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 IdleScreen

//! Minimal logging replacing `tracing` across the idle workspace.
//!
//! Provides level-filtered `error!`..`trace!` macros writing to stderr,
//! controlled by `RUST_LOG`. Supports three optional persistent sinks:
//! - Systemd journal datagram socket (`/run/systemd/journal/socket`)
//! - Syslog RFC 3164 datagram socket (`/dev/log`)
//! - Atomic rolling log file (`~/.local/state/idlescreen/idle-daemon.log`)

use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

pub mod sinks;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Level {
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    Trace = 5,
}

impl Level {
    fn from_name(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "error" => Some(Self::Error),
            "warn" | "warning" => Some(Self::Warn),
            "info" => Some(Self::Info),
            "debug" => Some(Self::Debug),
            "trace" => Some(Self::Trace),
            _ => None,
        }
    }

    fn priority(self) -> u8 {
        match self {
            Self::Error => 3,
            Self::Warn => 4,
            Self::Info => 6,
            Self::Debug | Self::Trace => 7,
        }
    }
}

static ENABLED: AtomicU8 = AtomicU8::new(Level::Warn as u8);
#[cfg(not(target_arch = "wasm32"))]
static JOURNALD: AtomicBool = AtomicBool::new(false);
#[cfg(not(target_arch = "wasm32"))]
static SYSLOG: AtomicBool = AtomicBool::new(false);
#[cfg(not(target_arch = "wasm32"))]
static FILE_LOG: AtomicBool = AtomicBool::new(false);

static IDENT: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

pub fn init(default: &str) {
    let spec = std::env::var("RUST_LOG").unwrap_or_else(|_| default.to_string());
    let mut max = 0u8;
    for part in spec.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let level = part
            .rsplit_once('=')
            .map_or_else(|| Level::from_name(part), |(_, l)| Level::from_name(l));
        if let Some(l) = level {
            max = max.max(l as u8);
        }
    }
    ENABLED.store(max, Ordering::Relaxed);
}

#[cfg(not(target_arch = "wasm32"))]
pub fn enable_journald(ident: &str) {
    set_ident(ident);
    JOURNALD.store(true, Ordering::Relaxed);
}

#[cfg(not(target_arch = "wasm32"))]
pub fn enable_syslog(ident: &str) {
    set_ident(ident);
    SYSLOG.store(true, Ordering::Relaxed);
}

#[cfg(not(target_arch = "wasm32"))]
pub fn enable_file(path: Option<&std::path::Path>) {
    sinks::file::init_file(path);
    FILE_LOG.store(true, Ordering::Relaxed);
}

#[inline]
fn set_ident(ident: &str) {
    match IDENT.write() {
        Ok(mut g) => *g = ident.to_string(),
        Err(e) => *e.into_inner() = ident.to_string(),
    }
}

#[inline]
fn get_ident() -> String {
    IDENT
        .read()
        .map(|s| s.clone())
        .unwrap_or_else(|e| e.into_inner().clone())
}

pub fn enabled(level: Level) -> bool {
    level as u8 <= ENABLED.load(Ordering::Relaxed)
}

pub fn emit(level: Level, target: &str, msg: std::fmt::Arguments<'_>) {
    if !enabled(level) {
        return;
    }
    let text = msg.to_string();
    let name = match level {
        Level::Error => "ERROR",
        Level::Warn => "WARN",
        Level::Info => "INFO",
        Level::Debug => "DEBUG",
        Level::Trace => "TRACE",
    };
    let line = format!("{name} {target}: {text}");
    let _ = writeln!(std::io::stderr().lock(), "{line}");
    #[cfg(not(target_arch = "wasm32"))]
    {
        if JOURNALD.load(Ordering::Relaxed) {
            sinks::journald::send(level.priority(), &get_ident(), &text);
        }
        if SYSLOG.load(Ordering::Relaxed) {
            let pri = sinks::syslog::rfc3164_pri(3, level.priority());
            sinks::syslog::send(pri, &get_ident(), &text);
        }
        if FILE_LOG.load(Ordering::Relaxed) {
            sinks::file::append(&line);
        }
    }
}

#[doc(hidden)]
#[macro_export]
macro_rules! __log_msg {
    (target: $t:literal, $($rest:tt)*) => {
        $crate::__log_msg!($($rest)*)
    };
    ($k:tt = % $v:expr, $($rest:tt)*) => {
        format!("{} = {}, ", stringify!($k), $v) + &$crate::__log_msg!($($rest)*)
    };
    ($k:tt = ? $v:expr, $($rest:tt)*) => {
        format!("{} = {:?}, ", stringify!($k), $v) + &$crate::__log_msg!($($rest)*)
    };
    ($k:tt = $v:expr, $($rest:tt)*) => {
        format!("{} = {}, ", stringify!($k), $v) + &$crate::__log_msg!($($rest)*)
    };
    ($k:ident, $($rest:tt)*) => {
        format!("{} = {}, ", stringify!($k), $k) + &$crate::__log_msg!($($rest)*)
    };
    (% $k:ident, $($rest:tt)*) => {
        format!("{} = {}, ", stringify!($k), $k) + &$crate::__log_msg!($($rest)*)
    };
    (? $k:ident, $($rest:tt)*) => {
        format!("{} = {:?}, ", stringify!($k), $k) + &$crate::__log_msg!($($rest)*)
    };
    ($fmt:literal $($rest:tt)*) => {
        format!($fmt $($rest)*)
    };
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        $crate::emit(
            $crate::Level::Error,
            module_path!(),
            format_args!("{}", $crate::__log_msg!($($arg)*)),
        )
    };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::emit(
            $crate::Level::Warn,
            module_path!(),
            format_args!("{}", $crate::__log_msg!($($arg)*)),
        )
    };
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        $crate::emit(
            $crate::Level::Info,
            module_path!(),
            format_args!("{}", $crate::__log_msg!($($arg)*)),
        )
    };
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        $crate::emit(
            $crate::Level::Debug,
            module_path!(),
            format_args!("{}", $crate::__log_msg!($($arg)*)),
        )
    };
}

#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {
        $crate::emit(
            $crate::Level::Trace,
            module_path!(),
            format_args!("{}", $crate::__log_msg!($($arg)*)),
        )
    };
}

#[cfg(test)]
mod tests;

#[cfg(target_arch = "wasm32")]
pub fn enable_journald(_ident: &str) {}
#[cfg(target_arch = "wasm32")]
pub fn enable_syslog(_ident: &str) {}
#[cfg(target_arch = "wasm32")]
pub fn enable_file(_path: Option<&std::path::Path>) {}
