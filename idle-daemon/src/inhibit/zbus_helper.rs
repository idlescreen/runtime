// SPDX-License-Identifier: MIT

//! Panic-safe zbus blocking call wrapper under resource exhaustion.

use std::panic::{self, AssertUnwindSafe, PanicHookInfo, catch_unwind};
use std::sync::Mutex;

static PANIC_HOOK_LOCK: Mutex<()> = Mutex::new(());

/// RAII guard that temporarily suppresses the global panic hook.
pub struct PanicHookGuard {
    prev_hook: Option<Box<dyn Fn(&PanicHookInfo<'_>) + Sync + Send + 'static>>,
}

impl PanicHookGuard {
    /// Takes the current panic hook and replaces it with a silent no-op hook.
    pub fn suppress() -> Self {
        let prev_hook = panic::take_hook();
        panic::set_hook(Box::new(|_| {}));
        Self {
            prev_hook: Some(prev_hook),
        }
    }
}

impl Drop for PanicHookGuard {
    fn drop(&mut self) {
        if let Some(prev) = self.prev_hook.take() {
            panic::set_hook(prev);
        }
    }
}

/// Executes a closure inside `catch_unwind` while suppressing the global panic hook.
pub fn suppress_and_catch<F, R>(f: F) -> std::thread::Result<R>
where
    F: FnOnce() -> R,
{
    let _lock = PANIC_HOOK_LOCK
        .lock()
        .unwrap_or_else(|p| crate::locks::poison_or_exit("lock", p));
    let _guard = PanicHookGuard::suppress();
    catch_unwind(AssertUnwindSafe(f))
}

/// Checks if at least `required` file descriptors are available to open.
pub fn check_fd_availability(required: usize) -> bool {
    let mut fds = Vec::with_capacity(required);
    let mut available = true;
    for _ in 0..required {
        match std::fs::File::open("/dev/null") {
            Ok(file) => fds.push(file),
            Err(_) => {
                available = false;
                break;
            }
        }
    }
    drop(fds);
    available
}

/// Executes a closure performing blocking `zbus` operations safely.
///
/// Pre-checks file descriptor availability, and wraps execution in
/// panic hook suppression + `catch_unwind` to intercept internal `zbus`
/// panics under FD limit exhaustion without writing panic traces to stderr.
/// Spawns a dedicated thread when invoked from inside a Tokio runtime thread
/// to avoid nested Tokio runtime conflicts.
pub fn safe_zbus_blocking<F, R>(f: F) -> Option<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    if !check_fd_availability(4) {
        return None;
    }
    let run = move || suppress_and_catch(f).ok();
    if tokio::runtime::Handle::try_current().is_ok() {
        std::thread::spawn(run).join().ok().flatten()
    } else {
        run()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic)] // Test panic helpers exercised below.
    use super::*;

    #[test]
    fn test_fd_availability_check() {
        assert!(check_fd_availability(1));
        assert!(check_fd_availability(4));
    }

    #[test]
    fn test_suppress_and_catch_panic() {
        let result = suppress_and_catch(|| {
            panic!("intentional panic for test");
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_safe_zbus_blocking_success() {
        let val = safe_zbus_blocking(|| 42);
        assert_eq!(val, Some(42));
    }

    #[test]
    fn test_safe_zbus_blocking_panic_handling() {
        let val = safe_zbus_blocking(|| {
            panic!("zbus runtime error panic simulation");
        });
        assert_eq!(val, None);
    }
}
