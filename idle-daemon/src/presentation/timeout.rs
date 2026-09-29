// SPDX-License-Identifier: MIT

use std::io;
use std::time::Duration;

/// Default per-IPC-call read timeout. The platform layer sets a
/// `read_timeout` / `write_timeout` on the UnixStream at session init;
/// if the saver doesn't respond in time, we treat it as hung and kill
/// the child. Operators can tighten via `IDLE_IPC_READ_TIMEOUT_MS`.
pub const DEFAULT_IPC_READ_TIMEOUT: Duration = Duration::from_millis(500);

/// Return the configured IPC read timeout, with fallback to default.
/// Reads from `IDLE_IPC_READ_TIMEOUT_MS` environment variable.
pub(crate) fn read_timeout() -> Duration {
    std::env::var("IDLE_IPC_READ_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_IPC_READ_TIMEOUT)
}

/// Classify an `std::io::Error` as a socket timeout (read/write deadline
/// elapsed). The platform layer sets a `read_timeout` / `write_timeout`
/// on the UnixStream at session init; on deadline the kernel returns
/// `TimedOut` (Unix) or `WouldBlock` (fallback). When we see either,
/// the peer hung inside an IPC command — `kill_child()` is the right
/// call.
pub(crate) fn is_timeout(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    )
}
