// SPDX-License-Identifier: MIT

//! Helpers for poisoned `Mutex`/`RwLock` guards.
//!
//! A thread that panics while holding a `Mutex`/`RwLock` poisons the lock.
//! Recovering from a poisoned lock by ignoring the error (the
//! `unwrap_or_else(|e| e.into_inner())` pattern) silently continues with
//! potentially-inconsistent state and is **fail-open**. Every call site in
//! `idle-dbus` should use [`poison_or_exit`] instead so the daemon aborts
//! cleanly and systemd can restart it.

/// Abort the daemon when a `Mutex`/`RwLock` is poisoned.
pub fn poison_or_exit<T>(name: &str, p: std::sync::PoisonError<T>) -> ! {
    eprintln!(
        "[idle-dbus] mutex/rwlock poisoned: lock={name} error={p}; \
         daemon state may be torn, aborting for clean restart"
    );
    std::process::abort();
}
