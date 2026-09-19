// SPDX-License-Identifier: MIT

//! Helpers for poisoned `Mutex`/`RwLock` guards.
//!
//! A thread that panics while holding a `Mutex`/`RwLock` poisons the lock.
//! Recovering from a poisoned lock by ignoring the error (the
//! `unwrap_or_else(|e| e.into_inner())` pattern) silently continues with
//! potentially-inconsistent state and is **fail-open**. Every call site in
//! `idle-daemon` should use [`poison_or_exit`] instead so the daemon aborts
//! cleanly and systemd can restart it.

/// Abort the daemon when a `Mutex`/`RwLock` is poisoned.
///
/// Use as: `self.foo.lock().unwrap_or_else(|p| poison_or_exit("foo", p))`.
///
/// The error is logged to the tracing subscriber (which journald captures
/// before `abort` runs destructors). We use `abort()` rather than `exit()` to
/// avoid invoking destructors on other threads while state may be torn.
pub fn poison_or_exit<T>(name: &str, p: std::sync::PoisonError<T>) -> ! {
    idle_log::error!(
        lock = name,
        error = %p,
        "mutex poisoned (holder panicked); daemon state may be torn, aborting for clean restart",
    );
    std::process::abort();
}

#[cfg(test)]
#[path = "locks_tests.rs"]
mod locks_tests;
