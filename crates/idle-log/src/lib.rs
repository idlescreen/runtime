// SPDX-License-Identifier: Apache-2.0
// perf: T3 · metric: crosses a process or socket boundary; dominated by IPC latency · check: review
// Copyright 2026 IdleScreen

//! Minimal logging replacing `tracing`/`tracing-subscriber`/
//! `tracing-journald` across the idle workspace: level-filtered
//! `error!`..`trace!` macros writing to stderr, controlled by `RUST_LOG`
//! (bare level or `target=level` list — the max enabled level wins, like
//! EnvFilter's global threshold). [`enable_journald`] additionally mirrors
//! records to the systemd journal via the native sd-journal datagram
//! protocol (`/run/systemd/journal/socket`).

use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

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
        // syslog priorities used by the journal: 3=err .. 7=debug.
        match self {
            Self::Error => 3,
            Self::Warn => 4,
            Self::Info => 6,
            Self::Debug | Self::Trace => 7,
        }
    }
}

/// Enabled threshold; `off` (0) disables everything.
static ENABLED: AtomicU8 = AtomicU8::new(Level::Warn as u8);
static JOURNALD: AtomicBool = AtomicBool::new(false);
static IDENT: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

/// Install the level filter from `RUST_LOG` (or `default` when unset).
/// Accepts `RUST_LOG=debug`, `RUST_LOG=idle=debug,zbus=warn`, `RUST_LOG=off`.
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

/// Mirror records to the systemd journal with `ident` as SYSLOG_IDENTIFIER.
/// Call when `JOURNAL_STREAM` is set (running under systemd), like the
/// previous `tracing_journald::layer()` wiring.
pub fn enable_journald(ident: &str) {
    // Recover from poisoning rather than dropping the ident entirely.
    match IDENT.write() {
        Ok(mut g) => *g = ident.to_string(),
        Err(e) => *e.into_inner() = ident.to_string(),
    }
    JOURNALD.store(true, Ordering::Relaxed);
}

pub fn enabled(level: Level) -> bool {
    level as u8 <= ENABLED.load(Ordering::Relaxed)
}

/// Emit one record (stderr always; journal too when enabled).
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
    let _ = writeln!(std::io::stderr().lock(), "{name} {target}: {text}");
    if JOURNALD.load(Ordering::Relaxed) {
        journald_send(level.priority(), &text);
    }
}

/// sd-journal over `/run/systemd/journal/socket`: newline-separated
/// `KEY=value` fields in a single datagram. Best-effort; failures ignored.
///
/// The socket is created once and reused. It used to be rebuilt per record —
/// a `socket()` syscall plus a `format!` for every log line, which at
/// `RUST_LOG=debug` on a busy path is a syscall storm on a daemon whose whole
/// job is to stay cheap. An unbound datagram socket can `send_to` repeatedly.
fn journald_socket() -> Option<&'static std::os::unix::net::UnixDatagram> {
    static SOCK: std::sync::OnceLock<Option<std::os::unix::net::UnixDatagram>> =
        std::sync::OnceLock::new();
    SOCK.get_or_init(|| {
        use std::os::unix::net::UnixDatagram;
        UnixDatagram::unbound().ok()
    })
    .as_ref()
}

fn journald_send(priority: u8, msg: &str) {
    let Some(sock) = journald_socket() else {
        return;
    };
    // A poisoned lock means some other thread panicked while setting IDENT.
    // Recover the value rather than silently logging with an empty
    // SYSLOG_IDENTIFIER, which makes records unattributable in the journal.
    let ident = IDENT
        .read()
        .map(|s| s.clone())
        .unwrap_or_else(|e| e.into_inner().clone());
    let payload = format!(
        "PRIORITY={priority}\nMESSAGE={}\nSYSLOG_IDENTIFIER={ident}\n",
        msg.replace('\n', " ")
    );
    let _ = sock.send_to(payload.as_bytes(), "/run/systemd/journal/socket");
}

// `#[macro_export]` rather than `pub use`: `warn` is a builtin attribute
// name and cannot be re-exported through a `use` path (E0659).
//
// The macros accept both plain `format!`-style messages and tracing's
// structured-field syntax (`field = %v` Display, `field = ?v` Debug,
// `field = v` Display, `target: "name"`) — fields are flattened into the
// message text as `k = v` pairs so call sites keep their information.

#[doc(hidden)]
#[macro_export]
macro_rules! __log_msg {
    // `target:` override — kept for source compatibility; the emitted
    // record target stays `module_path!()` like tracing's fmt default.
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
    // Shorthand field: `output_id,` means `output_id = output_id`.
    ($k:ident, $($rest:tt)*) => {
        format!("{} = {}, ", stringify!($k), $k) + &$crate::__log_msg!($($rest)*)
    };
    // Sigil shorthand: `%v,` / `?v,` mean `v = %v` / `v = ?v`.
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
